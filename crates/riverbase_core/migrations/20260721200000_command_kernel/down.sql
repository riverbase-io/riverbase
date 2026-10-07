DROP INDEX IF EXISTS riverbase_audit.idempotency_key_lease_idx;

ALTER TABLE riverbase_audit.idempotency_key
    DROP CONSTRAINT IF EXISTS idempotency_key_status_check;

UPDATE riverbase_audit.idempotency_key
SET status = 'in_flight'
WHERE status = 'failed';

ALTER TABLE riverbase_audit.idempotency_key
    ADD CONSTRAINT idempotency_key_status_check
    CHECK (status IN ('in_flight', 'completed'));

ALTER TABLE riverbase_audit.idempotency_key
    DROP COLUMN IF EXISTS error,
    DROP COLUMN IF EXISTS failed_at,
    DROP COLUMN IF EXISTS lease_expires_at,
    DROP COLUMN IF EXISTS owner_token;

DROP TABLE IF EXISTS riverbase_audit.outbox;
DROP TABLE IF EXISTS riverbase_audit.command_response;
DROP TABLE IF EXISTS riverbase_workflow.process_manager;
DROP SCHEMA IF EXISTS riverbase_workflow;
