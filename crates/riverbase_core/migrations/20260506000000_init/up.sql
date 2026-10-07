-- riverbase_audit log store schema and tables.

CREATE SCHEMA IF NOT EXISTS riverbase_audit;

DO $$ BEGIN
    CREATE TYPE riverbase_audit.activity_msg_type AS ENUM (
        'USER_ACTION', 'APP_REQUEST', 'SYSTEM_CALL'
    );
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
    CREATE TYPE riverbase_audit.command_status AS ENUM (
        'SUCCESS', 'CREATED', 'PENDING', 'RUNNING', 'DENIED', 'REJECTED', 'SUBMITTED',
        'APPLIED', 'ERRORED', 'RETRY_1', 'RETRY_2', 'RETRY_3', 'FAILED', 'CANCELED'
    );
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
    CREATE TYPE riverbase_audit.command_action AS ENUM (
        'CREATE', 'UPDATE', 'REMOVE'
    );
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
    CREATE TYPE riverbase_audit.domain_entity_type AS ENUM (
        'QUERY', 'EVENT', 'COMMAND', 'RESPONSE', 'MESSAGE', 'CONTEXT',
        'EVT_HANDLER', 'CMD_HANDLER', 'RESOURCE', 'ACTIVITY_LOG', 'MUTATION'
    );
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
    CREATE TYPE riverbase_audit.domain_transport AS ENUM (
        'SANIC', 'REDIS', 'KAFKA', 'FASTAPI', 'RABITTMQ', 'CLI', 'UNKNOWN'
    );
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE TABLE IF NOT EXISTS riverbase_audit.context_log (
    _id UUID PRIMARY KEY,
    _created TIMESTAMPTZ NOT NULL DEFAULT now(),
    _creator UUID,
    domain TEXT,
    revision INTEGER,
    realm UUID,
    dataset_id UUID,
    request_id UUID,
    user_id UUID,
    profile_id UUID,
    organization_id UUID,
    iam_roles TEXT[],
    session TEXT,
    timestamp TIMESTAMPTZ,
    transport riverbase_audit.domain_transport,
    source JSONB,
    headers JSONB
);

CREATE TABLE IF NOT EXISTS riverbase_audit.activity_log (
    _id UUID PRIMARY KEY,
    _created TIMESTAMPTZ NOT NULL DEFAULT now(),
    _creator UUID,
    _source TEXT,
    domain TEXT,
    identifier UUID,
    resource TEXT,
    domain_sid UUID,
    domain_iid UUID,
    message TEXT,
    msgtype riverbase_audit.activity_msg_type,
    msglabel TEXT,
    context UUID,
    src_cmd UUID,
    src_evt UUID,
    data JSONB,
    code INTEGER
);

CREATE TABLE IF NOT EXISTS riverbase_audit.event_log (
    _id UUID PRIMARY KEY,
    _created TIMESTAMPTZ NOT NULL DEFAULT now(),
    _creator UUID,
    domain TEXT,
    event TEXT,
    identifier UUID,
    resource TEXT,
    src_cmd UUID,
    args JSONB,
    data JSONB
);

CREATE TABLE IF NOT EXISTS riverbase_audit.message_log (
    _id UUID PRIMARY KEY,
    _created TIMESTAMPTZ NOT NULL DEFAULT now(),
    _creator UUID,
    domain TEXT,
    src_cmd UUID,
    message TEXT,
    data JSONB
);

CREATE TABLE IF NOT EXISTS riverbase_audit.command_log (
    _id UUID PRIMARY KEY,
    _created TIMESTAMPTZ NOT NULL DEFAULT now(),
    _creator UUID,
    domain TEXT,
    identifier UUID,
    resource TEXT,
    revision INTEGER,
    command TEXT,
    domain_sid UUID,
    domain_iid UUID,
    payload JSONB,
    context UUID NOT NULL,
    status riverbase_audit.command_status
);

CREATE TABLE IF NOT EXISTS riverbase_audit.command_queue (
    _id UUID PRIMARY KEY,
    _created TIMESTAMPTZ NOT NULL DEFAULT now(),
    _creator UUID,
    domain TEXT,
    identifier UUID,
    resource TEXT,
    selector__identifier UUID,
    selector__resource TEXT,
    context UUID NOT NULL,
    status riverbase_audit.command_status,
    transact UUID,
    data JSONB,
    stream_id UUID,
    cmd_action riverbase_audit.command_action,
    message TEXT,
    _domain TEXT,
    _kind riverbase_audit.domain_entity_type,
    _updated TIMESTAMPTZ,
    _vers INTEGER
);

CREATE TABLE IF NOT EXISTS riverbase_audit.update_queue (
    _id UUID PRIMARY KEY,
    _created TIMESTAMPTZ NOT NULL DEFAULT now(),
    _creator UUID,
    action TEXT,
    note TEXT,
    requester UUID,
    request_time TIMESTAMPTZ,
    commit_time TIMESTAMPTZ,
    status TEXT,
    committer UUID
);
