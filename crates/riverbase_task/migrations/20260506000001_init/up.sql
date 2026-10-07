-- riverbase_task job/worker schema and tables.

CREATE SCHEMA IF NOT EXISTS riverbase_task;

CREATE TABLE IF NOT EXISTS riverbase_task.worker (
    _id UUID PRIMARY KEY,
    _created TIMESTAMPTZ NOT NULL DEFAULT now(),
    _updated TIMESTAMPTZ,
    _creator UUID,
    _updater UUID,
    _realm UUID,
    _etag UUID,
    _deleted TIMESTAMPTZ,
    pid INTEGER,
    status TEXT,
    hostname TEXT,
    queue_name TEXT,
    jobs_complete INTEGER,
    jobs_failed INTEGER,
    jobs_retried INTEGER,
    jobs_queued INTEGER,
    start_time TIMESTAMPTZ,
    heart_beat TIMESTAMPTZ,
    stop_time TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS riverbase_task.worker_job (
    _id UUID PRIMARY KEY,
    _created TIMESTAMPTZ NOT NULL DEFAULT now(),
    _updated TIMESTAMPTZ,
    _creator UUID,
    _updater UUID,
    _realm UUID,
    _etag UUID,
    _deleted TIMESTAMPTZ,
    worker_id UUID REFERENCES riverbase_task.worker(_id),
    job_message TEXT,
    job_progress DOUBLE PRECISION,
    job_status TEXT,
    job_try INTEGER,
    score BIGINT,
    queue_name TEXT,
    function TEXT,
    args JSONB,
    kwargs JSONB,
    result JSONB,
    err_message TEXT,
    err_trace TEXT,
    enqueue_time TIMESTAMPTZ,
    start_time TIMESTAMPTZ,
    finish_time TIMESTAMPTZ,
    defer_time TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS riverbase_task.job_relation (
    _id UUID PRIMARY KEY,
    _created TIMESTAMPTZ NOT NULL DEFAULT now(),
    _updated TIMESTAMPTZ,
    _creator UUID,
    _updater UUID,
    _realm UUID,
    _etag UUID,
    _deleted TIMESTAMPTZ,
    job_id UUID,
    resource TEXT,
    resource_id UUID,
    domain TEXT
);
