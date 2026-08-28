BEGIN;

CREATE TABLE tos_versions (
    version INTEGER PRIMARY KEY CHECK (version > 0),
    document_url TEXT NOT NULL,
    document_sha256 BYTEA NOT NULL CHECK (octet_length(document_sha256) = 32),
    effective_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    is_current BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE UNIQUE INDEX tos_versions_one_current_idx
    ON tos_versions ((is_current))
    WHERE is_current;

CREATE TABLE players (
    id UUID PRIMARY KEY,
    discord_user_id BIGINT NOT NULL UNIQUE CHECK (discord_user_id > 0),
    status TEXT NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE', 'SOFT_FROZEN', 'HARD_FROZEN', 'DELETED')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ
);

CREATE TABLE tos_acceptances (
    player_id UUID NOT NULL REFERENCES players(id),
    tos_version INTEGER NOT NULL REFERENCES tos_versions(version),
    accepted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (player_id, tos_version)
);

CREATE TABLE deletion_cooldowns (
    identity_hmac BYTEA PRIMARY KEY CHECK (octet_length(identity_hmac) = 32),
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX deletion_cooldowns_expiry_idx ON deletion_cooldowns (expires_at);

CREATE TABLE operations (
    id UUID PRIMARY KEY,
    external_request_key TEXT NOT NULL UNIQUE,
    actor_discord_user_id BIGINT NOT NULL CHECK (actor_discord_user_id > 0),
    player_id UUID REFERENCES players(id),
    kind TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('PENDING', 'COMMITTED', 'CANCELLED', 'FAILED', 'REVERSED')),
    policy_version INTEGER NOT NULL CHECK (policy_version > 0),
    request_hash BYTEA NOT NULL CHECK (octet_length(request_hash) = 32),
    rng_root BYTEA NOT NULL CHECK (octet_length(rng_root) = 32),
    result JSONB,
    error_code TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    committed_at TIMESTAMPTZ
);

CREATE INDEX operations_actor_created_idx
    ON operations (actor_discord_user_id, created_at DESC);

CREATE TABLE player_balances (
    player_id UUID PRIMARY KEY REFERENCES players(id),
    wallet BIGINT NOT NULL DEFAULT 0 CHECK (wallet >= 0),
    bank BIGINT NOT NULL DEFAULT 0 CHECK (bank >= 0),
    liability BIGINT NOT NULL DEFAULT 0 CHECK (liability >= 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE ledger_transactions (
    id UUID PRIMARY KEY,
    operation_id UUID NOT NULL UNIQUE REFERENCES operations(id),
    kind TEXT NOT NULL,
    provenance JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE ledger_postings (
    transaction_id UUID NOT NULL REFERENCES ledger_transactions(id),
    sequence SMALLINT NOT NULL CHECK (sequence >= 0),
    player_id UUID REFERENCES players(id),
    account_kind TEXT NOT NULL CHECK (account_kind IN ('WALLET', 'BANK', 'LIABILITY', 'ESCROW', 'SYSTEM')),
    amount BIGINT NOT NULL CHECK (amount <> 0),
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    PRIMARY KEY (transaction_id, sequence)
);

CREATE INDEX ledger_postings_player_idx
    ON ledger_postings (player_id, transaction_id)
    WHERE player_id IS NOT NULL;

CREATE OR REPLACE FUNCTION graphite_forbid_ledger_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'Graphite ledger history is immutable; use a reversal/compensation transaction';
END;
$$;

CREATE TRIGGER ledger_transactions_immutable
BEFORE UPDATE OR DELETE ON ledger_transactions
FOR EACH ROW EXECUTE FUNCTION graphite_forbid_ledger_mutation();

CREATE TRIGGER ledger_postings_immutable
BEFORE UPDATE OR DELETE ON ledger_postings
FOR EACH ROW EXECUTE FUNCTION graphite_forbid_ledger_mutation();

CREATE OR REPLACE FUNCTION graphite_assert_ledger_balanced()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    total NUMERIC;
BEGIN
    SELECT COALESCE(SUM(amount::NUMERIC), 0)
      INTO total
      FROM ledger_postings
     WHERE transaction_id = NEW.transaction_id;

    IF total <> 0 THEN
        RAISE EXCEPTION 'Unbalanced Graphite ledger transaction % (sum=%)', NEW.transaction_id, total;
    END IF;

    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER ledger_postings_balanced
AFTER INSERT ON ledger_postings
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION graphite_assert_ledger_balanced();

CREATE TABLE outbox_events (
    id UUID PRIMARY KEY,
    operation_id UUID NOT NULL REFERENCES operations(id),
    topic TEXT NOT NULL,
    payload JSONB NOT NULL,
    state TEXT NOT NULL DEFAULT 'PENDING' CHECK (state IN ('PENDING', 'DISPATCHED', 'FAILED')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    dispatched_at TIMESTAMPTZ,
    UNIQUE (operation_id, topic)
);

CREATE INDEX outbox_pending_idx
    ON outbox_events (state, created_at)
    WHERE state = 'PENDING';

CREATE TABLE item_definitions (
    key TEXT PRIMARY KEY,
    category TEXT NOT NULL,
    stackable BOOLEAN NOT NULL,
    active BOOLEAN NOT NULL DEFAULT TRUE,
    definition_version INTEGER NOT NULL DEFAULT 1 CHECK (definition_version > 0),
    data JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE item_instances (
    id UUID PRIMARY KEY,
    definition_key TEXT NOT NULL REFERENCES item_definitions(key),
    owner_player_id UUID NOT NULL REFERENCES players(id),
    created_by_operation_id UUID NOT NULL REFERENCES operations(id),
    location TEXT NOT NULL CHECK (location IN ('EQUIPPED', 'TOOL_LOCKER', 'ITEM_BAG', 'CATCH_BAG', 'TEMP_OVERFLOW', 'MARKET_ESCROW', 'TRADE_ESCROW', 'PROCESSING_OUTPUT', 'TRASH_RECOVERY', 'JOB_RESERVATION')),
    is_starter BOOLEAN NOT NULL DEFAULT FALSE,
    is_account_bound BOOLEAN NOT NULL DEFAULT FALSE,
    is_tradeable BOOLEAN NOT NULL DEFAULT TRUE,
    is_sellable BOOLEAN NOT NULL DEFAULT TRUE,
    is_discardable BOOLEAN NOT NULL DEFAULT TRUE,
    is_enchantable BOOLEAN NOT NULL DEFAULT TRUE,
    is_upgradeable BOOLEAN NOT NULL DEFAULT TRUE,
    is_unbreakable BOOLEAN NOT NULL DEFAULT FALSE,
    is_repairable BOOLEAN NOT NULL DEFAULT TRUE,
    current_durability BIGINT CHECK (current_durability IS NULL OR current_durability >= 0),
    max_durability BIGINT CHECK (max_durability IS NULL OR max_durability > 0),
    is_broken BOOLEAN NOT NULL DEFAULT FALSE,
    state JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((current_durability IS NULL) = (max_durability IS NULL)),
    CHECK (current_durability IS NULL OR current_durability <= max_durability)
);

CREATE UNIQUE INDEX starter_definition_once_per_player_idx
    ON item_instances (owner_player_id, definition_key)
    WHERE is_starter;

CREATE TABLE equipment_slots (
    player_id UUID NOT NULL REFERENCES players(id),
    slot TEXT NOT NULL CHECK (slot IN ('PICKAXE', 'SWORD', 'FISHING_ROD', 'ARMOR_HELMET', 'ARMOR_CHEST', 'ARMOR_LEGS', 'ARMOR_BOOTS', 'TOTEM')),
    item_instance_id UUID NOT NULL UNIQUE REFERENCES item_instances(id),
    PRIMARY KEY (player_id, slot)
);

CREATE TABLE asset_events (
    id UUID PRIMARY KEY,
    operation_id UUID NOT NULL REFERENCES operations(id),
    player_id UUID NOT NULL REFERENCES players(id),
    event_kind TEXT NOT NULL,
    payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX asset_events_player_created_idx
    ON asset_events (player_id, created_at DESC);

CREATE OR REPLACE FUNCTION graphite_forbid_asset_event_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'Graphite asset event history is immutable';
END;
$$;

CREATE TRIGGER asset_events_immutable
BEFORE UPDATE OR DELETE ON asset_events
FOR EACH ROW EXECUTE FUNCTION graphite_forbid_asset_event_mutation();

CREATE TABLE bank_lots (
    id UUID PRIMARY KEY,
    player_id UUID NOT NULL REFERENCES players(id),
    principal_remaining BIGINT NOT NULL CHECK (principal_remaining >= 0),
    interest_remainder BIGINT NOT NULL DEFAULT 0 CHECK (interest_remainder >= 0),
    deposited_at TIMESTAMPTZ NOT NULL,
    created_by_operation_id UUID NOT NULL REFERENCES operations(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX bank_lots_fifo_idx
    ON bank_lots (player_id, deposited_at, id)
    WHERE principal_remaining > 0;

CREATE TABLE guild_settings (
    guild_id BIGINT PRIMARY KEY CHECK (guild_id > 0),
    text_prefix TEXT,
    xp_modifier_bps INTEGER NOT NULL DEFAULT 10000 CHECK (xp_modifier_bps >= 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (text_prefix IS NULL OR (char_length(text_prefix) BETWEEN 1 AND 16))
);

INSERT INTO item_definitions (key, category, stackable, data) VALUES
    ('equipment.pickaxe.wood.starter', 'PICKAXE', FALSE, '{"tier":"WOOD","capability":"C0","roll_min":1,"roll_max":5,"ordinary_durability":700,"starter_unbreakable":true}'::jsonb),
    ('equipment.sword.wood.starter', 'SWORD', FALSE, '{"tier":"WOOD","base_damage":3,"ordinary_durability":600,"starter_unbreakable":true}'::jsonb),
    ('equipment.rod.basic.starter', 'FISHING_ROD', FALSE, '{"tier":"WOOD","line_strength":6,"ordinary_durability":600,"starter_unbreakable":true}'::jsonb),
    ('equipment.armor.leather.helmet.starter', 'ARMOR', FALSE, '{"tier":"LEATHER","slot":"ARMOR_HELMET","integrity":1}'::jsonb),
    ('equipment.armor.leather.chest.starter', 'ARMOR', FALSE, '{"tier":"LEATHER","slot":"ARMOR_CHEST","integrity":3}'::jsonb),
    ('equipment.armor.leather.legs.starter', 'ARMOR', FALSE, '{"tier":"LEATHER","slot":"ARMOR_LEGS","integrity":2}'::jsonb),
    ('equipment.armor.leather.boots.starter', 'ARMOR', FALSE, '{"tier":"LEATHER","slot":"ARMOR_BOOTS","integrity":1}'::jsonb);

COMMIT;
