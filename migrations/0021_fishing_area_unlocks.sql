BEGIN;

CREATE TABLE player_fishing_area_unlocks (
    player_id UUID NOT NULL REFERENCES players(id),
    area TEXT NOT NULL CHECK (area IN ('RIVER', 'LAKE', 'COAST', 'DEEP_SEA', 'ABYSS')),
    granted_by_operation_id UUID NOT NULL REFERENCES operations(id),
    unlocked_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (player_id, area)
);

CREATE INDEX player_fishing_area_unlocks_operation_idx
    ON player_fishing_area_unlocks (granted_by_operation_id);

COMMIT;
