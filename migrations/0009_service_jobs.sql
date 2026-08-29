BEGIN;

-- Per-job reservations supersede the aggregate JOB_RESERVATION Item Stack location.
-- Keeping that legacy write path available would merge concurrent jobs by
-- (player, definition, version, location) and destroy reservation ownership.
ALTER TABLE item_stacks
    ADD CONSTRAINT item_stacks_no_legacy_job_reservation
    CHECK (location <> 'JOB_RESERVATION');

CREATE TABLE service_jobs (
    id UUID PRIMARY KEY,
    operation_id UUID NOT NULL UNIQUE REFERENCES operations(id),
    player_id UUID NOT NULL REFERENCES players(id),
    service_kind TEXT NOT NULL CHECK (
        char_length(service_kind) > 0
        AND service_kind = btrim(service_kind)
    ),
    policy_version INTEGER NOT NULL CHECK (policy_version > 0),
    state TEXT NOT NULL CHECK (state IN ('RUNNING', 'COMPLETED', 'CANCELLED', 'FAILED')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX service_jobs_player_state_created_idx
    ON service_jobs (player_id, state, created_at, id);

CREATE TABLE service_job_stack_reservations (
    job_id UUID NOT NULL REFERENCES service_jobs(id),
    role TEXT NOT NULL CHECK (role IN ('INPUT', 'FUEL')),
    definition_key TEXT NOT NULL,
    definition_version INTEGER NOT NULL,
    quantity BIGINT NOT NULL CHECK (quantity > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (job_id, role, definition_key, definition_version),
    FOREIGN KEY (definition_key, definition_version)
        REFERENCES item_definition_versions(key, version)
);

CREATE INDEX service_job_stack_reservations_definition_idx
    ON service_job_stack_reservations (definition_key, definition_version, job_id);

CREATE OR REPLACE FUNCTION graphite_guard_service_job_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Graphite service jobs are retained for audit provenance';
    END IF;

    IF NEW.id IS DISTINCT FROM OLD.id
       OR NEW.operation_id IS DISTINCT FROM OLD.operation_id
       OR NEW.player_id IS DISTINCT FROM OLD.player_id
       OR NEW.service_kind IS DISTINCT FROM OLD.service_kind
       OR NEW.policy_version IS DISTINCT FROM OLD.policy_version
       OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN
        RAISE EXCEPTION 'Graphite service-job identity/provenance is immutable';
    END IF;

    IF NEW.state IS DISTINCT FROM OLD.state THEN
        IF OLD.state <> 'RUNNING'
           OR NEW.state NOT IN ('COMPLETED', 'CANCELLED', 'FAILED') THEN
            RAISE EXCEPTION 'Invalid Graphite service-job state transition % -> %', OLD.state, NEW.state;
        END IF;
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER service_jobs_guard_mutation
BEFORE UPDATE OR DELETE ON service_jobs
FOR EACH ROW EXECUTE FUNCTION graphite_guard_service_job_mutation();

CREATE OR REPLACE FUNCTION graphite_forbid_service_job_reservation_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'Graphite service-job initial reservation provenance is immutable';
END;
$$;

CREATE TRIGGER service_job_stack_reservations_immutable
BEFORE UPDATE OR DELETE ON service_job_stack_reservations
FOR EACH ROW EXECUTE FUNCTION graphite_forbid_service_job_reservation_mutation();

COMMIT;
