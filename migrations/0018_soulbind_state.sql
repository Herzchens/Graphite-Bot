BEGIN;

CREATE TABLE item_instance_soulbind_state (
    item_instance_id UUID PRIMARY KEY REFERENCES item_instances(id) ON DELETE CASCADE,
    is_soulbound BOOLEAN NOT NULL,
    rebind_not_before TIMESTAMPTZ NULL,
    CONSTRAINT soulbind_state_shape CHECK (
        (is_soulbound AND rebind_not_before IS NULL)
        OR (NOT is_soulbound AND rebind_not_before IS NOT NULL)
    )
);

CREATE OR REPLACE FUNCTION graphite_validate_soulbind_state_identity()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.item_instance_id IS DISTINCT FROM OLD.item_instance_id THEN
        RAISE EXCEPTION 'Graphite SoulBind state ItemInstance identity is immutable';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER soulbind_state_identity_immutable
BEFORE UPDATE ON item_instance_soulbind_state
FOR EACH ROW EXECUTE FUNCTION graphite_validate_soulbind_state_identity();

COMMIT;
