BEGIN;

CREATE TABLE bank_withdrawals (
    operation_id UUID PRIMARY KEY REFERENCES operations(id),
    player_id UUID NOT NULL REFERENCES players(id),
    gross_amount BIGINT NOT NULL CHECK (gross_amount > 0),
    fee_amount BIGINT NOT NULL CHECK (fee_amount >= 0 AND fee_amount < gross_amount),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX bank_withdrawals_player_created_idx
    ON bank_withdrawals (player_id, created_at DESC);

CREATE OR REPLACE FUNCTION graphite_forbid_bank_withdrawal_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'Graphite bank withdrawal history is immutable';
END;
$$;

CREATE TRIGGER bank_withdrawals_immutable
BEFORE UPDATE OR DELETE ON bank_withdrawals
FOR EACH ROW EXECUTE FUNCTION graphite_forbid_bank_withdrawal_mutation();

COMMIT;
