BEGIN;

CREATE TABLE item_instance_embedded_enchants (
    item_instance_id UUID NOT NULL REFERENCES item_instances(id) ON DELETE CASCADE,
    enchant_key TEXT NOT NULL,
    level SMALLINT NOT NULL,
    PRIMARY KEY (item_instance_id, enchant_key),
    CONSTRAINT embedded_enchant_key_nonempty CHECK (
        char_length(enchant_key) BETWEEN 1 AND 64
        AND enchant_key = btrim(enchant_key)
    ),
    CONSTRAINT embedded_enchant_level_supported CHECK (level BETWEEN 1 AND 10)
);

CREATE OR REPLACE FUNCTION graphite_assert_embedded_enchant_parent_shape(target_item UUID)
RETURNS VOID
LANGUAGE plpgsql
AS $$
DECLARE
    item_category TEXT;
    item_enchantable BOOLEAN;
    item_starter BOOLEAN;
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM item_instance_embedded_enchants
         WHERE item_instance_id = target_item
    ) THEN
        RETURN;
    END IF;

    SELECT d.category, i.is_enchantable, i.is_starter
      INTO item_category, item_enchantable, item_starter
      FROM item_instances i
      JOIN item_definition_versions d
        ON d.key = i.definition_key
       AND d.version = i.definition_version
     WHERE i.id = target_item;

    IF NOT FOUND OR item_category NOT IN ('PICKAXE', 'SWORD', 'FISHING_ROD', 'ARMOR') THEN
        RAISE EXCEPTION 'Graphite embedded enchant state requires an equipment ItemDefinition version';
    END IF;

    IF NOT item_enchantable OR item_starter THEN
        RAISE EXCEPTION 'Graphite embedded enchant state requires a non-starter enchantable ItemInstance';
    END IF;
END;
$$;

CREATE OR REPLACE FUNCTION graphite_validate_embedded_enchant_write()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'UPDATE' AND (
        NEW.item_instance_id IS DISTINCT FROM OLD.item_instance_id
        OR NEW.enchant_key IS DISTINCT FROM OLD.enchant_key
    ) THEN
        RAISE EXCEPTION 'Graphite embedded enchant row identity is immutable';
    END IF;

    PERFORM graphite_assert_embedded_enchant_parent_shape(NEW.item_instance_id);
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER embedded_enchant_write_consistency
AFTER INSERT OR UPDATE ON item_instance_embedded_enchants
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION graphite_validate_embedded_enchant_write();

CREATE OR REPLACE FUNCTION graphite_validate_embedded_enchant_item_repin()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    PERFORM graphite_assert_embedded_enchant_parent_shape(NEW.id);
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER item_instances_embedded_enchant_consistency
AFTER UPDATE OF definition_key, definition_version, is_enchantable, is_starter ON item_instances
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION graphite_validate_embedded_enchant_item_repin();

COMMIT;
