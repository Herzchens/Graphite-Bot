BEGIN;

ALTER TABLE pending_asset_deliveries
    DROP CONSTRAINT pending_asset_deliveries_unique_instance_shape,
    ADD CONSTRAINT pending_asset_deliveries_unique_instance_shape CHECK (
        item_instance_id IS NULL
        OR (
            quantity = 1
            AND desired_location IN ('CATCH_BAG', 'ITEM_BAG')
        )
    );

INSERT INTO item_definitions (
    key, category, stackable, active, definition_version, rarity, stack_limit, unit_weight_grams, data
)
VALUES (
    'enchant.book.finished',
    'ENCHANT_BOOK',
    FALSE,
    TRUE,
    1,
    'COMMON',
    NULL,
    NULL,
    '{"book_kind":"FINISHED_ENCHANT_BOOK"}'::jsonb
);

INSERT INTO item_definition_versions (
    key, version, category, stackable, rarity, stack_limit, unit_weight_grams, data
)
SELECT
    key,
    definition_version,
    category,
    stackable,
    rarity,
    stack_limit,
    unit_weight_grams,
    data
FROM item_definitions
WHERE key = 'enchant.book.finished';

CREATE TABLE item_instance_finished_enchant_books (
    item_instance_id UUID PRIMARY KEY REFERENCES item_instances(id) ON DELETE CASCADE,
    enchant_key TEXT NOT NULL,
    level SMALLINT NOT NULL,
    CONSTRAINT finished_enchant_book_key_nonempty CHECK (
        char_length(enchant_key) BETWEEN 1 AND 64
        AND enchant_key = btrim(enchant_key)
    ),
    CONSTRAINT finished_enchant_book_level_supported CHECK (level BETWEEN 1 AND 10)
);

CREATE OR REPLACE FUNCTION graphite_assert_finished_enchant_book_parent_shape(target_item UUID)
RETURNS VOID
LANGUAGE plpgsql
AS $$
DECLARE
    item_definition_key TEXT;
    item_category TEXT;
    item_stackable BOOLEAN;
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM item_instance_finished_enchant_books
         WHERE item_instance_id = target_item
    ) THEN
        RETURN;
    END IF;

    SELECT i.definition_key, d.category, d.stackable
      INTO item_definition_key, item_category, item_stackable
      FROM item_instances i
      JOIN item_definition_versions d
        ON d.key = i.definition_key
       AND d.version = i.definition_version
     WHERE i.id = target_item
     FOR UPDATE OF i;

    IF NOT FOUND
       OR item_definition_key <> 'enchant.book.finished'
       OR item_category <> 'ENCHANT_BOOK'
       OR item_stackable THEN
        RAISE EXCEPTION 'Graphite finished Enchant Book state requires the canonical unique Book ItemDefinition';
    END IF;
END;
$$;

CREATE OR REPLACE FUNCTION graphite_lock_finished_enchant_book_parent()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    PERFORM 1
      FROM item_instances
     WHERE id = NEW.item_instance_id
     FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION 'Graphite finished Enchant Book state requires an existing parent ItemInstance';
    END IF;

    IF TG_OP = 'UPDATE' THEN
        RAISE EXCEPTION 'Graphite finished Enchant Book identity is immutable';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER finished_enchant_book_lock_parent
BEFORE INSERT OR UPDATE ON item_instance_finished_enchant_books
FOR EACH ROW EXECUTE FUNCTION graphite_lock_finished_enchant_book_parent();

CREATE OR REPLACE FUNCTION graphite_validate_finished_enchant_book_write()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    PERFORM graphite_assert_finished_enchant_book_parent_shape(NEW.item_instance_id);
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER finished_enchant_book_write_consistency
AFTER INSERT ON item_instance_finished_enchant_books
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION graphite_validate_finished_enchant_book_write();

CREATE OR REPLACE FUNCTION graphite_validate_finished_enchant_book_item_shape()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $book$
BEGIN
    IF NEW.definition_key = 'enchant.book.finished'
       AND NOT EXISTS (
           SELECT 1
             FROM item_instance_finished_enchant_books
            WHERE item_instance_id = NEW.id
       ) THEN
        RAISE EXCEPTION 'Graphite Finished Enchant Book ItemInstance requires typed book identity';
    END IF;

    PERFORM graphite_assert_finished_enchant_book_parent_shape(NEW.id);
    RETURN NULL;
END;
$book$;

CREATE CONSTRAINT TRIGGER item_instances_finished_enchant_book_insert_consistency
AFTER INSERT ON item_instances
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION graphite_validate_finished_enchant_book_item_shape();

CREATE CONSTRAINT TRIGGER item_instances_finished_enchant_book_update_consistency
AFTER UPDATE OF definition_key, definition_version ON item_instances
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION graphite_validate_finished_enchant_book_item_shape();


CREATE OR REPLACE FUNCTION graphite_forbid_finished_enchant_book_repin()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $book_repin$
BEGIN
    IF OLD.definition_key = 'enchant.book.finished'
       AND (
           NEW.definition_key <> OLD.definition_key
           OR NEW.definition_version <> OLD.definition_version
       ) THEN
        RAISE EXCEPTION 'Graphite Finished Enchant Book definition pin is immutable';
    END IF;

    RETURN NEW;
END;
$book_repin$;

CREATE TRIGGER item_instances_finished_enchant_book_repin_guard
BEFORE UPDATE OF definition_key, definition_version ON item_instances
FOR EACH ROW EXECUTE FUNCTION graphite_forbid_finished_enchant_book_repin();

CREATE OR REPLACE FUNCTION graphite_validate_finished_enchant_book_delete()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $book_delete$
BEGIN
    IF EXISTS (
        SELECT 1
          FROM item_instances
         WHERE id = OLD.item_instance_id
           AND definition_key = 'enchant.book.finished'
    ) THEN
        RAISE EXCEPTION 'Graphite Finished Enchant Book typed identity cannot be detached from a live Book ItemInstance';
    END IF;

    RETURN NULL;
END;
$book_delete$;

CREATE CONSTRAINT TRIGGER finished_enchant_book_delete_consistency
AFTER DELETE ON item_instance_finished_enchant_books
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION graphite_validate_finished_enchant_book_delete();

COMMIT;
