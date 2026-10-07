use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::types::{ActivityMsgType, CommandLogStatus, DomainTransport, QueryLogStatus};
use crate::base::CommandId;

/// Shared audit columns on all `riverbase_audit` log tables (`DomainLogBaseModel`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRowMeta {
    /// Row identifier.
    pub id: Uuid,
    /// Creation timestamp.
    pub created: DateTime<Utc>,
    /// Creating principal identifier, if known.
    pub creator: Option<Uuid>,
}

/// Request / command execution context (`context_log`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextLogRecord {
    #[serde(flatten)]
    /// Shared metadata.
    pub meta: LogRowMeta,
    /// Domain namespace.
    pub domain: Option<String>,
    /// Aggregate revision.
    pub revision: Option<i32>,
    /// Deprecated portal-hash column. Writers leave this unused; tenant identity is `_tenant`.
    pub realm: Option<Uuid>,
    /// Dataset id.
    pub dataset_id: Option<Uuid>,
    /// Request id.
    pub request_id: Option<Uuid>,
    /// User id.
    pub user_id: Option<Uuid>,
    /// Active profile identifier.
    pub profile_id: Option<Uuid>,
    /// Organization id.
    pub organization_id: Option<Uuid>,
    /// Tenant data scope (`_tenant`). Not auth `realm`.
    pub tenant: Option<Uuid>,
    /// Iam roles.
    pub iam_roles: Vec<String>,
    /// Session.
    pub session: Option<String>,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
    /// Transport.
    pub transport: DomainTransport,
    /// Source.
    pub source: Value,
    /// Headers.
    pub headers: Option<Value>,
}

/// Command envelope (`command_log`), aligned with `riverbase.domain.command.CommandBundle`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandLogRecord {
    #[serde(flatten)]
    /// Shared metadata.
    pub meta: LogRowMeta,
    /// Domain namespace.
    pub domain: String,
    /// Identifier.
    pub identifier: Option<Uuid>,
    /// Resource.
    pub resource: String,
    /// Aggregate revision.
    pub revision: i32,
    /// Command.
    pub command: String,
    /// Domain sid.
    pub domain_sid: Option<Uuid>,
    /// Domain iid.
    pub domain_iid: Option<Uuid>,
    /// Command or event payload.
    pub payload: Value,
    /// Execution context identifier.
    pub context: Uuid,
    /// Status.
    pub status: CommandLogStatus,
    /// Tenant data scope (`_tenant`).
    pub tenant: Option<Uuid>,
}

/// Domain event (`event_log`), aligned with `riverbase.domain.event.EventRecord`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventLogRecord {
    #[serde(flatten)]
    /// Shared metadata.
    pub meta: LogRowMeta,
    /// Domain namespace.
    pub domain: Option<String>,
    /// Event.
    pub event: String,
    /// Identifier.
    pub identifier: Option<Uuid>,
    /// Resource.
    pub resource: Option<String>,
    /// Src cmd.
    pub src_cmd: Uuid,
    /// Args.
    pub args: Value,
    /// Data.
    pub data: Value,
    /// Tenant data scope (`_tenant`).
    pub tenant: Option<Uuid>,
}

/// Integration message (`message_log`), aligned with `riverbase.domain.message.MessageBundle`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageLogRecord {
    #[serde(flatten)]
    /// Shared metadata.
    pub meta: LogRowMeta,
    /// Domain namespace.
    pub domain: String,
    /// Src cmd.
    pub src_cmd: Uuid,
    /// Message.
    pub message: String,
    /// Data.
    pub data: Value,
    /// Tenant data scope (`_tenant`).
    pub tenant: Option<Uuid>,
}

/// Activity (`activity_log`), aligned with `riverbase.domain.activity.ActivityLog`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityLogRecord {
    #[serde(flatten)]
    /// Shared metadata.
    pub meta: LogRowMeta,
    /// Source.
    pub source: Option<String>,
    /// Domain namespace.
    pub domain: String,
    /// Identifier.
    pub identifier: Option<Uuid>,
    /// Resource.
    pub resource: String,
    /// Domain sid.
    pub domain_sid: Option<Uuid>,
    /// Domain iid.
    pub domain_iid: Option<Uuid>,
    /// Message.
    pub message: String,
    /// Msgtype.
    pub msgtype: ActivityMsgType,
    /// Msglabel.
    pub msglabel: String,
    /// Execution context identifier.
    pub context: Option<Uuid>,
    /// Src cmd.
    pub src_cmd: Option<Uuid>,
    /// Src evt.
    pub src_evt: Option<Uuid>,
    /// Data.
    pub data: Option<Value>,
    /// Code.
    pub code: i32,
    /// Tenant data scope (`_tenant`).
    pub tenant: Option<Uuid>,
}

/// Query envelope (`query_log`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryLogRecord {
    #[serde(flatten)]
    /// Shared metadata.
    pub meta: LogRowMeta,
    /// Domain namespace.
    pub domain: String,
    /// Resource.
    pub resource: String,
    /// Access.
    pub access: String,
    /// Identifier.
    pub identifier: Option<Uuid>,
    /// Domain sid.
    pub domain_sid: Option<Uuid>,
    /// Domain iid.
    pub domain_iid: Option<Uuid>,
    /// Request.
    pub request: Value,
    /// Execution context identifier.
    pub context: Uuid,
    /// Status.
    pub status: QueryLogStatus,
    /// Result count.
    pub result_count: Option<i32>,
    /// Error code.
    pub error_code: Option<String>,
    /// Tenant data scope (`_tenant`).
    pub tenant: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Response record structure.
pub struct ResponseRecord {
    /// Cmd id.
    pub cmd_id: CommandId,
    /// Command or event payload.
    pub payload: Value,
}

/// Durable integration-message delivery record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxRecord {
    /// Row identifier.
    pub id: Uuid,
    /// Creation timestamp.
    pub created: DateTime<Utc>,
    /// Src cmd.
    pub src_cmd: Uuid,
    /// Topic.
    pub topic: String,
    /// Command or event payload.
    pub payload: Value,
    /// Attempts.
    pub attempts: i32,
}
