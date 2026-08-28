BEGIN;

ALTER TABLE item_definitions
    ADD COLUMN rarity TEXT NOT NULL DEFAULT 'COMMON'
        CHECK (rarity IN ('COMMON', 'UNCOMMON', 'RARE', 'EPIC', 'LEGENDARY', 'MYTHIC')),
    ADD COLUMN stack_limit BIGINT,
    ADD COLUMN unit_weight_grams BIGINT,
    ADD CONSTRAINT item_definitions_stack_limit_shape CHECK (
        (stackable AND stack_limit IS NOT NULL AND stack_limit > 0)
        OR (NOT stackable AND stack_limit IS NULL)
    ),
    ADD CONSTRAINT item_definitions_unit_weight_positive CHECK (
        unit_weight_grams IS NULL OR unit_weight_grams > 0
    );

CREATE TABLE item_definition_versions (
    key TEXT NOT NULL REFERENCES item_definitions(key),
    version INTEGER NOT NULL CHECK (version > 0),
    category TEXT NOT NULL,
    stackable BOOLEAN NOT NULL,
    rarity TEXT NOT NULL CHECK (rarity IN ('COMMON', 'UNCOMMON', 'RARE', 'EPIC', 'LEGENDARY', 'MYTHIC')),
    stack_limit BIGINT,
    unit_weight_grams BIGINT,
    data JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (key, version),
    CHECK (
        (stackable AND stack_limit IS NOT NULL AND stack_limit > 0)
        OR (NOT stackable AND stack_limit IS NULL)
    ),
    CHECK (unit_weight_grams IS NULL OR unit_weight_grams > 0)
);

INSERT INTO item_definition_versions (
    key, version, category, stackable, rarity, stack_limit, unit_weight_grams, data
)
SELECT key, definition_version, category, stackable, rarity, stack_limit, unit_weight_grams, data
  FROM item_definitions;

CREATE OR REPLACE FUNCTION graphite_forbid_item_definition_version_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'Graphite item definition versions are immutable; create a newer version';
END;
$$;

CREATE TRIGGER item_definition_versions_immutable
BEFORE UPDATE OR DELETE ON item_definition_versions
FOR EACH ROW EXECUTE FUNCTION graphite_forbid_item_definition_version_mutation();

CREATE OR REPLACE FUNCTION graphite_forbid_item_definition_delete()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'Graphite item definitions are inactivated, never deleted';
END;
$$;

CREATE TRIGGER item_definitions_no_delete
BEFORE DELETE ON item_definitions
FOR EACH ROW EXECUTE FUNCTION graphite_forbid_item_definition_delete();

ALTER TABLE item_instances
    ADD COLUMN definition_version INTEGER NOT NULL DEFAULT 1,
    ADD COLUMN catch_weight_grams BIGINT,
    ADD CONSTRAINT item_instances_definition_version_fk
        FOREIGN KEY (definition_key, definition_version)
        REFERENCES item_definition_versions(key, version),
    ADD CONSTRAINT item_instances_catch_weight_positive
        CHECK (catch_weight_grams IS NULL OR catch_weight_grams > 0);

CREATE TABLE player_storage_profiles (
    player_id UUID PRIMARY KEY REFERENCES players(id),
    item_bag_level BIGINT NOT NULL DEFAULT 0 CHECK (item_bag_level >= 0),
    catch_bag_level BIGINT NOT NULL DEFAULT 0 CHECK (catch_bag_level >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO player_storage_profiles (player_id)
SELECT id FROM players
ON CONFLICT (player_id) DO NOTHING;

CREATE OR REPLACE FUNCTION graphite_initialize_player_storage_profile()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    INSERT INTO player_storage_profiles (player_id)
    VALUES (NEW.id)
    ON CONFLICT (player_id) DO NOTHING;
    RETURN NEW;
END;
$$;

CREATE TRIGGER players_initialize_storage_profile
AFTER INSERT ON players
FOR EACH ROW EXECUTE FUNCTION graphite_initialize_player_storage_profile();

CREATE TABLE item_stacks (
    player_id UUID NOT NULL REFERENCES players(id),
    definition_key TEXT NOT NULL,
    definition_version INTEGER NOT NULL,
    location TEXT NOT NULL CHECK (location IN (
        'ITEM_BAG', 'TEMP_OVERFLOW', 'MARKET_ESCROW', 'TRADE_ESCROW',
        'PROCESSING_OUTPUT', 'JOB_RESERVATION'
    )),
    quantity BIGINT NOT NULL CHECK (quantity > 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (player_id, definition_key, definition_version, location),
    FOREIGN KEY (definition_key, definition_version)
        REFERENCES item_definition_versions(key, version)
);

CREATE INDEX item_stacks_player_location_idx
    ON item_stacks (player_id, location, definition_key, definition_version);

CREATE OR REPLACE FUNCTION graphite_validate_stack_definition()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    is_stackable BOOLEAN;
    version_stack_limit BIGINT;
BEGIN
    SELECT stackable, stack_limit
      INTO is_stackable, version_stack_limit
      FROM item_definition_versions
     WHERE key = NEW.definition_key
       AND version = NEW.definition_version;

    IF NOT FOUND OR NOT is_stackable OR version_stack_limit IS NULL OR version_stack_limit <= 0 THEN
        RAISE EXCEPTION 'Graphite item stack requires a stackable versioned definition';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER item_stacks_validate_definition
BEFORE INSERT OR UPDATE OF definition_key, definition_version ON item_stacks
FOR EACH ROW EXECUTE FUNCTION graphite_validate_stack_definition();

CREATE TABLE pending_asset_deliveries (
    id UUID PRIMARY KEY,
    operation_id UUID NOT NULL UNIQUE REFERENCES operations(id),
    player_id UUID NOT NULL REFERENCES players(id),
    definition_key TEXT NOT NULL,
    definition_version INTEGER NOT NULL,
    quantity BIGINT NOT NULL CHECK (quantity > 0),
    desired_location TEXT NOT NULL CHECK (desired_location IN ('ITEM_BAG', 'CATCH_BAG', 'TOOL_LOCKER')),
    reason TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'PENDING' CHECK (state IN ('PENDING', 'CLAIMED', 'CANCELLED')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved_at TIMESTAMPTZ,
    FOREIGN KEY (definition_key, definition_version)
        REFERENCES item_definition_versions(key, version)
);

CREATE INDEX pending_asset_deliveries_player_state_idx
    ON pending_asset_deliveries (player_id, state, created_at);

CREATE OR REPLACE FUNCTION graphite_assert_equipment_item_consistency(target_item UUID)
RETURNS VOID
LANGUAGE plpgsql
AS $$
DECLARE
    owner_id UUID;
    item_location TEXT;
    item_category TEXT;
    definition_data JSONB;
    slot_count INTEGER;
    slot_owner UUID;
    slot_name TEXT;
    expected_slot TEXT;
BEGIN
    SELECT i.owner_player_id, i.location, d.category, d.data
      INTO owner_id, item_location, item_category, definition_data
      FROM item_instances i
      JOIN item_definition_versions d
        ON d.key = i.definition_key
       AND d.version = i.definition_version
     WHERE i.id = target_item;

    IF NOT FOUND THEN
        RETURN;
    END IF;

    SELECT COUNT(*)
      INTO slot_count
      FROM equipment_slots
     WHERE item_instance_id = target_item;

    SELECT player_id, slot
      INTO slot_owner, slot_name
      FROM equipment_slots
     WHERE item_instance_id = target_item;

    IF item_location = 'EQUIPPED' THEN
        IF slot_count <> 1 OR slot_owner IS DISTINCT FROM owner_id THEN
            RAISE EXCEPTION 'Equipped Graphite item % must have exactly one owner-matching equipment slot', target_item;
        END IF;

        expected_slot := CASE item_category
            WHEN 'PICKAXE' THEN 'PICKAXE'
            WHEN 'SWORD' THEN 'SWORD'
            WHEN 'FISHING_ROD' THEN 'FISHING_ROD'
            WHEN 'TOTEM' THEN 'TOTEM'
            WHEN 'ARMOR' THEN definition_data->>'slot'
            ELSE NULL
        END;

        IF expected_slot IS NULL OR slot_name IS DISTINCT FROM expected_slot THEN
            RAISE EXCEPTION 'Graphite item % is incompatible with equipment slot %', target_item, slot_name;
        END IF;
    ELSIF slot_count <> 0 THEN
        RAISE EXCEPTION 'Non-equipped Graphite item % cannot remain referenced by equipment_slots', target_item;
    END IF;
END;
$$;

CREATE OR REPLACE FUNCTION graphite_validate_equipment_slot_change()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP <> 'INSERT' THEN
        PERFORM graphite_assert_equipment_item_consistency(OLD.item_instance_id);
    END IF;
    IF TG_OP <> 'DELETE' THEN
        PERFORM graphite_assert_equipment_item_consistency(NEW.item_instance_id);
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER equipment_slots_consistency
AFTER INSERT OR UPDATE OR DELETE ON equipment_slots
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION graphite_validate_equipment_slot_change();

CREATE OR REPLACE FUNCTION graphite_validate_equipped_item_change()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    PERFORM graphite_assert_equipment_item_consistency(NEW.id);
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER item_instances_equipment_consistency
AFTER INSERT OR UPDATE OF owner_player_id, location, definition_key, definition_version ON item_instances
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION graphite_validate_equipped_item_change();

COMMIT;
