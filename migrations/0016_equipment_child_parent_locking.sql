BEGIN;

-- Cross-row equipment state invariants must serialize on the parent ItemInstance.
-- Deferred constraint triggers alone can otherwise admit write skew when one
-- transaction inserts child state while another concurrently changes the parent
-- shape and both validate before either transaction becomes visible to the other.
CREATE OR REPLACE FUNCTION graphite_assert_equipment_structural_state_shape(target_item UUID)
RETURNS VOID
LANGUAGE plpgsql
AS $$
DECLARE
    item_category TEXT;
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM item_instance_equipment_structural_state
         WHERE item_instance_id = target_item
    ) THEN
        RETURN;
    END IF;

    SELECT d.category
      INTO item_category
      FROM item_instances i
      JOIN item_definition_versions d
        ON d.key = i.definition_key
       AND d.version = i.definition_version
     WHERE i.id = target_item
     FOR UPDATE OF i;

    IF NOT FOUND OR item_category NOT IN ('PICKAXE', 'SWORD', 'FISHING_ROD', 'ARMOR') THEN
        RAISE EXCEPTION 'Graphite structural equipment state requires an equipment ItemDefinition version';
    END IF;
END;
$$;

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
     WHERE i.id = target_item
     FOR UPDATE OF i;

    IF NOT FOUND OR item_category NOT IN ('PICKAXE', 'SWORD', 'FISHING_ROD', 'ARMOR') THEN
        RAISE EXCEPTION 'Graphite embedded enchant state requires an equipment ItemDefinition version';
    END IF;

    IF NOT item_enchantable OR item_starter THEN
        RAISE EXCEPTION 'Graphite embedded enchant state requires a non-starter enchantable ItemInstance';
    END IF;
END;
$$;

COMMIT;
