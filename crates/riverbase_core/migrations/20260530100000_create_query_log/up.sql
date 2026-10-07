CREATE TABLE IF NOT EXISTS riverbase_audit.query_log (
    _id UUID PRIMARY KEY,
    _created TIMESTAMPTZ NOT NULL DEFAULT now(),
    _creator UUID,
    domain TEXT,
    resource TEXT,
    access TEXT NOT NULL,
    identifier UUID,
    domain_sid UUID,
    domain_iid UUID,
    request JSONB,
    context UUID NOT NULL,
    status riverbase_audit.command_status,
    result_count INTEGER,
    error_code TEXT
);
