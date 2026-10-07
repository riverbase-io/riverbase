use serde::Deserialize;

/// Per-channel switches for audit log persistence.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AuditLogConfig {
    /// Command envelope log (`riverbase_audit.command_log`).
    pub command: bool,
    /// Request context log (`riverbase_audit.context_log`).
    pub context: bool,
    /// Domain event log (`riverbase_audit.event_log`).
    pub event: bool,
    /// Integration message log (`riverbase_audit.message_log`).
    pub message: bool,
    /// Activity log (`riverbase_audit.activity_log`).
    pub activity: bool,
    /// Command response log.
    pub response: bool,
    /// Query request log (`riverbase_audit.query_log`).
    pub query: bool,
    /// Command idempotency store (`riverbase_audit.idempotency_key`).
    pub idempotency: bool,
}

impl Default for AuditLogConfig {
    fn default() -> Self {
        Self {
            command: true,
            context: true,
            event: true,
            message: true,
            activity: true,
            response: true,
            query: false,
            idempotency: true,
        }
    }
}

impl AuditLogConfig {
    /// All channels enabled.
    pub fn all_enabled() -> Self {
        Self::default()
    }

    /// All channels disabled (no-op stores).
    pub fn all_disabled() -> Self {
        Self {
            command: false,
            context: false,
            event: false,
            message: false,
            activity: false,
            response: false,
            query: false,
            idempotency: false,
        }
    }
}

/// Parse boolean env values (`true`/`1`/`yes` vs `false`/`0`/`no`).
pub fn parse_bool_env(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}
