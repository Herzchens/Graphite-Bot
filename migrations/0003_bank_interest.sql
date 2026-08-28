BEGIN;

ALTER TABLE players
    ADD COLUMN rebirth_count BIGINT NOT NULL DEFAULT 0 CHECK (rebirth_count >= 0);

ALTER TABLE operations
    ALTER COLUMN actor_discord_user_id DROP NOT NULL;

-- `bank_lots.interest_remainder` remains reserved for backward schema compatibility;
-- authoritative fractional entitlement is account-level in `bank_interest_state`.

CREATE TABLE bank_interest_state (
    player_id UUID PRIMARY KEY REFERENCES players(id),
    remainder_q32 BIGINT NOT NULL DEFAULT 0
        CHECK (remainder_q32 >= 0 AND remainder_q32 < 4294967296000000),
    last_accrual_day DATE NOT NULL DEFAULT ((now() AT TIME ZONE 'UTC')::date),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO bank_interest_state (player_id)
SELECT player_id
  FROM player_balances
ON CONFLICT (player_id) DO NOTHING;

CREATE OR REPLACE FUNCTION graphite_initialize_bank_interest_state()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    INSERT INTO bank_interest_state (player_id)
    VALUES (NEW.player_id)
    ON CONFLICT (player_id) DO NOTHING;
    RETURN NEW;
END;
$$;

CREATE TRIGGER player_balances_initialize_bank_interest
AFTER INSERT ON player_balances
FOR EACH ROW EXECUTE FUNCTION graphite_initialize_bank_interest_state();

CREATE INDEX bank_interest_due_idx
    ON bank_interest_state (last_accrual_day, player_id);

COMMIT;
