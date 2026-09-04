BEGIN;

CREATE TABLE catch_bag_stack_lots (
    id UUID PRIMARY KEY,
    player_id UUID NOT NULL REFERENCES players(id),
    definition_key TEXT NOT NULL,
    definition_version INTEGER NOT NULL,
    created_by_operation_id UUID NOT NULL REFERENCES operations(id),
    quantity BIGINT NOT NULL CHECK (quantity > 0),
    total_weight_grams BIGINT NOT NULL CHECK (total_weight_grams > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (definition_key, definition_version)
        REFERENCES item_definition_versions(key, version)
);

CREATE INDEX catch_bag_stack_lots_player_created_idx
    ON catch_bag_stack_lots (player_id, created_at, id);

CREATE OR REPLACE FUNCTION graphite_validate_catch_bag_stack_lot()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    definition_stackable BOOLEAN;
    operation_player_id UUID;
BEGIN
    SELECT stackable
      INTO definition_stackable
      FROM item_definition_versions
     WHERE key = NEW.definition_key
       AND version = NEW.definition_version;

    IF NOT FOUND OR NOT definition_stackable THEN
        RAISE EXCEPTION 'CatchBag stack lot requires a stackable versioned definition';
    END IF;

    SELECT player_id
      INTO operation_player_id
      FROM operations
     WHERE id = NEW.created_by_operation_id;

    IF NOT FOUND OR operation_player_id IS DISTINCT FROM NEW.player_id THEN
        RAISE EXCEPTION 'CatchBag stack lot provenance operation must target the owning player';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER catch_bag_stack_lots_validate
BEFORE INSERT ON catch_bag_stack_lots
FOR EACH ROW EXECUTE FUNCTION graphite_validate_catch_bag_stack_lot();

CREATE OR REPLACE FUNCTION graphite_forbid_catch_bag_stack_lot_update()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'CatchBag stack lots are immutable while present; remove the whole lot through an owning operation';
END;
$$;

CREATE TRIGGER catch_bag_stack_lots_immutable
BEFORE UPDATE ON catch_bag_stack_lots
FOR EACH ROW EXECUTE FUNCTION graphite_forbid_catch_bag_stack_lot_update();

COMMIT;
