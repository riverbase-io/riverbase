CREATE TABLE IF NOT EXISTS riverbase_audit.idempotency_key (
    key TEXT NOT NULL,
    namespace TEXT NOT NULL,
    command TEXT NOT NULL,
    actor UUID,
    request_hash TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('in_flight', 'completed')),
    response JSONB,
    cmd_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    PRIMARY KEY (namespace, command, key)
);

CREATE INDEX IF NOT EXISTS idempotency_key_expires_at_idx
    ON riverbase_audit.idempotency_key (expires_at)
    WHERE expires_at IS NOT NULL;
