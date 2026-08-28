BEGIN;

CREATE TABLE player_progression (
    player_id UUID PRIMARY KEY REFERENCES players(id),
    account_xp BIGINT NOT NULL DEFAULT 0 CHECK (account_xp BETWEEN 0 AND 172370),
    activity_xp_points BIGINT NOT NULL DEFAULT 0 CHECK (activity_xp_points >= 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO player_progression (player_id)
SELECT id
  FROM players
 WHERE status <> 'DELETED'
ON CONFLICT (player_id) DO NOTHING;

CREATE OR REPLACE FUNCTION graphite_initialize_player_progression()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    INSERT INTO player_progression (player_id)
    VALUES (NEW.id)
    ON CONFLICT (player_id) DO NOTHING;
    RETURN NEW;
END;
$$;

CREATE TRIGGER players_initialize_progression
AFTER INSERT ON players
FOR EACH ROW EXECUTE FUNCTION graphite_initialize_player_progression();

CREATE TABLE progression_events (
    id UUID PRIMARY KEY,
    operation_id UUID NOT NULL UNIQUE REFERENCES operations(id),
    player_id UUID NOT NULL REFERENCES players(id),
    event_kind TEXT NOT NULL CHECK (event_kind IN (
        'ACCOUNT_XP_GRANTED',
        'ACTIVITY_XP_GRANTED',
        'ACTIVITY_XP_SPENT',
        'ACTIVITY_XP_LOST',
        'REBIRTH'
    )),
    payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX progression_events_player_created_idx
    ON progression_events (player_id, created_at DESC);

CREATE OR REPLACE FUNCTION graphite_forbid_progression_event_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'Graphite progression event history is immutable';
END;
$$;

CREATE TRIGGER progression_events_immutable
BEFORE UPDATE OR DELETE ON progression_events
FOR EACH ROW EXECUTE FUNCTION graphite_forbid_progression_event_mutation();

COMMIT;
