BEGIN;

CREATE TABLE smelting_job_runtimes (
    job_id UUID PRIMARY KEY REFERENCES service_jobs(id),
    requested_units BIGINT NOT NULL CHECK (requested_units > 0),
    accepted_units BIGINT NOT NULL CHECK (
        accepted_units > 0
        AND accepted_units <= requested_units
    ),
    fuel_kind TEXT NOT NULL CHECK (fuel_kind IN ('COAL', 'WOOD_LOG')),
    reserved_fuel_items BIGINT NOT NULL CHECK (reserved_fuel_items > 0),
    effective_unit_micros BIGINT NOT NULL CHECK (effective_unit_micros > 0),
    modifier_snapshot JSONB NOT NULL CHECK (jsonb_typeof(modifier_snapshot) = 'object'),
    started_at TIMESTAMPTZ NOT NULL,
    completes_at TIMESTAMPTZ NOT NULL,
    CHECK (completes_at > started_at)
);

CREATE INDEX smelting_job_runtimes_due_idx
    ON smelting_job_runtimes (completes_at, job_id);

CREATE OR REPLACE FUNCTION graphite_forbid_smelting_runtime_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'Graphite Smelting runtime snapshots are immutable after Confirm';
END;
$$;

CREATE TRIGGER smelting_job_runtimes_immutable
BEFORE UPDATE OR DELETE ON smelting_job_runtimes
FOR EACH ROW EXECUTE FUNCTION graphite_forbid_smelting_runtime_mutation();

COMMIT;
