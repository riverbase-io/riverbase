-- Tenant directory. `_id` is the tenant id. `resource` + `resource_id` name the
-- backing record (organization, profile, user, ...). Nothing writes this yet.

CREATE TABLE IF NOT EXISTS riverbase_audit.tenant (
    _id UUID PRIMARY KEY,
    _created TIMESTAMPTZ NOT NULL DEFAULT now(),
    _updated TIMESTAMPTZ NOT NULL DEFAULT now(),
    _creator UUID,
    _updater UUID,
    _etag UUID NOT NULL DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    resource TEXT NOT NULL,
    resource_id UUID NOT NULL,
    active BOOLEAN NOT NULL DEFAULT true
);

CREATE UNIQUE INDEX IF NOT EXISTS tenant_resource_uidx
    ON riverbase_audit.tenant (resource, resource_id);

CREATE INDEX IF NOT EXISTS tenant_name_idx
    ON riverbase_audit.tenant (name);
