BEGIN;

ALTER TABLE item_definition_versions
    ADD COLUMN is_ordinary_equipment BOOLEAN NOT NULL DEFAULT FALSE,
    ADD CONSTRAINT item_definition_versions_ordinary_equipment_shape CHECK (
        NOT is_ordinary_equipment
        OR (
            NOT stackable
            AND category IN ('PICKAXE', 'SWORD', 'FISHING_ROD', 'ARMOR')
        )
    );

COMMIT;
