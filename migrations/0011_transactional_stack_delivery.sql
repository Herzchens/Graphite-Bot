BEGIN;

ALTER TABLE asset_events
    ADD COLUMN mutation_key TEXT
        CHECK (
            mutation_key IS NULL
            OR (char_length(mutation_key) BETWEEN 1 AND 128)
        );

CREATE UNIQUE INDEX asset_events_operation_mutation_key_idx
    ON asset_events (operation_id, mutation_key)
    WHERE mutation_key IS NOT NULL;

ALTER TABLE pending_asset_deliveries
    DROP CONSTRAINT pending_asset_deliveries_operation_id_key;

ALTER TABLE pending_asset_deliveries
    ADD COLUMN mutation_key TEXT NOT NULL DEFAULT 'primary'
        CHECK (char_length(mutation_key) BETWEEN 1 AND 128);

ALTER TABLE pending_asset_deliveries
    ADD CONSTRAINT pending_asset_deliveries_operation_mutation_key_key
        UNIQUE (operation_id, mutation_key);

COMMIT;
