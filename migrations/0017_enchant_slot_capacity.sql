BEGIN;

-- Enchant slot unlocks are durable per-ItemInstance structural state. The active
-- specification freezes currently-unlocked capacity by family (Normal/class
-- 4..=6, Special/universal 3..=6) and preserves unlocked slots across tier
-- promotion. Persist only those authoritative capacities; placement/conflict
-- policy remains derived from the canonical Services catalog and is not
-- duplicated into PostgreSQL.
ALTER TABLE item_instance_equipment_structural_state
    ADD COLUMN normal_enchant_slot_capacity SMALLINT NOT NULL DEFAULT 4,
    ADD COLUMN special_enchant_slot_capacity SMALLINT NOT NULL DEFAULT 3,
    ADD CONSTRAINT equipment_structural_state_normal_enchant_slot_capacity_supported CHECK (
        normal_enchant_slot_capacity BETWEEN 4 AND 6
    ),
    ADD CONSTRAINT equipment_structural_state_special_enchant_slot_capacity_supported CHECK (
        special_enchant_slot_capacity BETWEEN 3 AND 6
    );

COMMIT;
