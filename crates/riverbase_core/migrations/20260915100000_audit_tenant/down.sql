DROP INDEX IF EXISTS riverbase_audit.query_log_tenant_idx;
DROP INDEX IF EXISTS riverbase_audit.message_log_tenant_idx;
DROP INDEX IF EXISTS riverbase_audit.activity_log_tenant_idx;
DROP INDEX IF EXISTS riverbase_audit.event_log_tenant_idx;
DROP INDEX IF EXISTS riverbase_audit.command_log_tenant_idx;
DROP INDEX IF EXISTS riverbase_audit.context_log_tenant_idx;

ALTER TABLE riverbase_audit.query_log    DROP COLUMN IF EXISTS _tenant;
ALTER TABLE riverbase_audit.message_log  DROP COLUMN IF EXISTS _tenant;
ALTER TABLE riverbase_audit.activity_log DROP COLUMN IF EXISTS _tenant;
ALTER TABLE riverbase_audit.event_log    DROP COLUMN IF EXISTS _tenant;
ALTER TABLE riverbase_audit.command_log  DROP COLUMN IF EXISTS _tenant;
ALTER TABLE riverbase_audit.context_log  DROP COLUMN IF EXISTS _tenant;
