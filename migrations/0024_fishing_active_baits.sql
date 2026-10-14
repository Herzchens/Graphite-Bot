BEGIN;

CREATE TABLE player_active_fishing_baits (
    player_id UUID NOT NULL REFERENCES players(id),
    bait_kind TEXT NOT NULL CHECK (
        bait_kind IN ('SCHOOL', 'QUALITY', 'RARE', 'TREASURE', 'STURDY')
    ),
    definition_key TEXT NOT NULL,
    definition_version INTEGER NOT NULL CHECK (definition_version > 0),
    activated_by_operation_id UUID NOT NULL REFERENCES operations(id),
    activated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (player_id, bait_kind),
    UNIQUE (player_id, definition_key, definition_version),
    FOREIGN KEY (definition_key, definition_version)
        REFERENCES item_definition_versions(key, version),
    CHECK (
        (bait_kind = 'SCHOOL' AND definition_key = 'fishing.bait.school')
        OR (bait_kind = 'QUALITY' AND definition_key = 'fishing.bait.quality')
        OR (bait_kind = 'RARE' AND definition_key = 'fishing.bait.rare')
        OR (bait_kind = 'TREASURE' AND definition_key = 'fishing.bait.treasure')
        OR (bait_kind = 'STURDY' AND definition_key = 'fishing.bait.sturdy')
    )
);

COMMIT;
