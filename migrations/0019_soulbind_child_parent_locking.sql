BEGIN;

CREATE OR REPLACE FUNCTION graphite_lock_soulbind_parent()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    PERFORM 1
      FROM item_instances
     WHERE id = NEW.item_instance_id
     FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION 'Graphite SoulBind state requires an existing parent ItemInstance';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER soulbind_state_lock_parent
BEFORE INSERT OR UPDATE ON item_instance_soulbind_state
FOR EACH ROW EXECUTE FUNCTION graphite_lock_soulbind_parent();

COMMIT;
