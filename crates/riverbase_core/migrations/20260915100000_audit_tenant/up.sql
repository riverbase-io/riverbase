-- Tenant data scope on audit channels. Do not reuse context_log.realm (auth/portal).

ALTER TABLE riverbase_audit.context_log  ADD COLUMN IF NOT EXISTS _tenant UUID;
ALTER TABLE riverbase_audit.command_log  ADD COLUMN IF NOT EXISTS _tenant UUID;
ALTER TABLE riverbase_audit.event_log    ADD COLUMN IF NOT EXISTS _tenant UUID;
ALTER TABLE riverbase_audit.activity_log ADD COLUMN IF NOT EXISTS _tenant UUID;
ALTER TABLE riverbase_audit.message_log  ADD COLUMN IF NOT EXISTS _tenant UUID;
ALTER TABLE riverbase_audit.query_log    ADD COLUMN IF NOT EXISTS _tenant UUID;

CREATE INDEX IF NOT EXISTS context_log_tenant_idx  ON riverbase_audit.context_log (_tenant);
CREATE INDEX IF NOT EXISTS command_log_tenant_idx  ON riverbase_audit.command_log (_tenant);
CREATE INDEX IF NOT EXISTS event_log_tenant_idx    ON riverbase_audit.event_log (_tenant);
CREATE INDEX IF NOT EXISTS activity_log_tenant_idx ON riverbase_audit.activity_log (_tenant);
CREATE INDEX IF NOT EXISTS message_log_tenant_idx  ON riverbase_audit.message_log (_tenant);
CREATE INDEX IF NOT EXISTS query_log_tenant_idx    ON riverbase_audit.query_log (_tenant);
