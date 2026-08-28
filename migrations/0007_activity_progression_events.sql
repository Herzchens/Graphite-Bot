BEGIN;

ALTER TABLE progression_events
    DROP CONSTRAINT IF EXISTS progression_events_operation_id_key;

ALTER TABLE progression_events
    ADD COLUMN mutation_key TEXT NOT NULL DEFAULT 'primary'
        CHECK (char_length(mutation_key) BETWEEN 1 AND 128);

ALTER TABLE progression_events
    ADD CONSTRAINT progression_events_operation_mutation_key_key
        UNIQUE (operation_id, mutation_key);

COMMIT;
