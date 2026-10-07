CREATE TABLE IF NOT EXISTS riverbase_audit.command_response (
    cmd_id TEXT PRIMARY KEY,
    payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS riverbase_audit.outbox (
    _id UUID PRIMARY KEY,
    _created TIMESTAMPTZ NOT NULL DEFAULT now(),
    src_cmd UUID NOT NULL REFERENCES riverbase_audit.command_log (_id) ON DELETE CASCADE,
    topic TEXT NOT NULL,
    payload JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'publishing', 'published', 'dead_letter')),
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    published_at TIMESTAMPTZ,
    last_error TEXT
);

CREATE INDEX IF NOT EXISTS outbox_delivery_idx
    ON riverbase_audit.outbox (status, next_attempt_at, _created)
    WHERE status IN ('pending', 'publishing');

ALTER TABLE riverbase_audit.idempotency_key
    DROP CONSTRAINT IF EXISTS idempotency_key_status_check;

ALTER TABLE riverbase_audit.idempotency_key
    ADD COLUMN IF NOT EXISTS owner_token UUID,
    ADD COLUMN IF NOT EXISTS lease_expires_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS failed_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS error JSONB;

ALTER TABLE riverbase_audit.idempotency_key
    ADD CONSTRAINT idempotency_key_status_check
    CHECK (status IN ('in_flight', 'completed', 'failed'));

CREATE INDEX IF NOT EXISTS idempotency_key_lease_idx
    ON riverbase_audit.idempotency_key (lease_expires_at)
    WHERE status = 'in_flight';

CREATE SCHEMA IF NOT EXISTS riverbase_workflow;

CREATE TABLE IF NOT EXISTS riverbase_workflow.process_manager (
    _id UUID PRIMARY KEY,
    workflow_type TEXT NOT NULL,
    correlation_key TEXT NOT NULL,
    state JSONB NOT NULL,
    status TEXT NOT NULL
        CHECK (status IN ('pending', 'running', 'retrying', 'compensating',
                          'compensated', 'failed', 'completed')),
    completed_steps TEXT[] NOT NULL DEFAULT '{}',
    version BIGINT NOT NULL DEFAULT 0,
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMPTZ,
    last_error JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workflow_type, correlation_key)
);

CREATE INDEX IF NOT EXISTS process_manager_due_idx
    ON riverbase_workflow.process_manager (status, next_attempt_at)
    WHERE status IN ('pending', 'retrying', 'compensating');
