BEGIN;

CREATE TABLE item_instance_equipment_structural_state (
    item_instance_id UUID PRIMARY KEY REFERENCES item_instances(id) ON DELETE CASCADE,
    creation_roll_numerator NUMERIC NOT NULL,
    creation_roll_denominator NUMERIC NOT NULL,
    upgrade_level NUMERIC NOT NULL DEFAULT 0,
    CONSTRAINT equipment_structural_state_creation_roll_numerator_integer CHECK (
        creation_roll_numerator = trunc(creation_roll_numerator)
    ),
    CONSTRAINT equipment_structural_state_creation_roll_numerator_u64 CHECK (
        creation_roll_numerator BETWEEN 0 AND 18446744073709551615::NUMERIC
    ),
    CONSTRAINT equipment_structural_state_creation_roll_denominator_integer CHECK (
        creation_roll_denominator = trunc(creation_roll_denominator)
    ),
    CONSTRAINT equipment_structural_state_creation_roll_denominator_u64 CHECK (
        creation_roll_denominator BETWEEN 1 AND 18446744073709551615::NUMERIC
    ),
    CONSTRAINT equipment_structural_state_creation_roll_unit_interval CHECK (
        creation_roll_numerator <= creation_roll_denominator
    ),
    CONSTRAINT equipment_structural_state_creation_roll_reduced CHECK (
        gcd(creation_roll_numerator, creation_roll_denominator) = 1
    ),
    CONSTRAINT equipment_structural_state_upgrade_level_integer CHECK (
        upgrade_level = trunc(upgrade_level)
    ),
    CONSTRAINT equipment_structural_state_upgrade_level_u64 CHECK (
        upgrade_level BETWEEN 0 AND 18446744073709551615::NUMERIC
    )
);

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
     WHERE i.id = target_item;

    IF NOT FOUND OR item_category NOT IN ('PICKAXE', 'SWORD', 'FISHING_ROD', 'ARMOR') THEN
        RAISE EXCEPTION 'Graphite structural equipment state requires an equipment ItemDefinition version';
    END IF;
END;
$$;

CREATE OR REPLACE FUNCTION graphite_validate_equipment_structural_state_write()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'UPDATE' AND (
        NEW.item_instance_id IS DISTINCT FROM OLD.item_instance_id
        OR NEW.creation_roll_numerator IS DISTINCT FROM OLD.creation_roll_numerator
        OR NEW.creation_roll_denominator IS DISTINCT FROM OLD.creation_roll_denominator
    ) THEN
        RAISE EXCEPTION 'Graphite Creation Roll is immutable after structural state creation';
    END IF;

    PERFORM graphite_assert_equipment_structural_state_shape(NEW.item_instance_id);
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER equipment_structural_state_write_consistency
AFTER INSERT OR UPDATE ON item_instance_equipment_structural_state
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION graphite_validate_equipment_structural_state_write();

CREATE OR REPLACE FUNCTION graphite_guard_equipment_structural_state_delete()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF EXISTS (
        SELECT 1
          FROM item_instances
         WHERE id = OLD.item_instance_id
    ) THEN
        RAISE EXCEPTION 'Graphite structural equipment state cannot be deleted while its ItemInstance exists';
    END IF;
    RETURN OLD;
END;
$$;

CREATE TRIGGER equipment_structural_state_delete_guard
BEFORE DELETE ON item_instance_equipment_structural_state
FOR EACH ROW EXECUTE FUNCTION graphite_guard_equipment_structural_state_delete();

CREATE OR REPLACE FUNCTION graphite_validate_structural_state_item_repin()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    PERFORM graphite_assert_equipment_structural_state_shape(NEW.id);
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER item_instances_structural_state_consistency
AFTER UPDATE OF definition_key, definition_version ON item_instances
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION graphite_validate_structural_state_item_repin();

COMMIT;
