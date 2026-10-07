use std::io::Write;

use diesel::deserialize::{self, FromSql};
use diesel::expression::AsExpression;
use diesel::pg::{Pg, PgValue};
use diesel::serialize::{self, Output, ToSql};
use serde::{Deserialize, Serialize};

/// Mirrors `riverbase_audit.domain_transport`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, AsExpression)]
#[diesel(sql_type = DomainTransportSql)]
pub enum DomainTransport {
    /// Sanic.
    Sanic,
    /// Redis.
    Redis,
    /// Kafka.
    Kafka,
    /// Fast api.
    FastApi,
    /// Rabbit mq.
    RabbitMq,
    /// Cli.
    Cli,
    /// Unknown.
    Unknown,
}

impl DomainTransport {
    /// Borrow as r.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sanic => "SANIC",
            Self::Redis => "REDIS",
            Self::Kafka => "KAFKA",
            Self::FastApi => "FASTAPI",
            Self::RabbitMq => "RABITTMQ",
            Self::Cli => "CLI",
            Self::Unknown => "UNKNOWN",
        }
    }
}

impl Default for DomainTransport {
    fn default() -> Self {
        Self::FastApi
    }
}

#[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
#[diesel(postgres_type(name = "domain_transport", schema = "riverbase_audit"))]
/// Domain transport sql; structure.
pub struct DomainTransportSql;

impl ToSql<DomainTransportSql, Pg> for DomainTransport {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        out.write_all(self.as_str().as_bytes())?;
        Ok(serialize::IsNull::No)
    }
}

impl FromSql<DomainTransportSql, Pg> for DomainTransport {
    fn from_sql(value: PgValue<'_>) -> deserialize::Result<Self> {
        match value.as_bytes() {
            b"SANIC" => Ok(Self::Sanic),
            b"REDIS" => Ok(Self::Redis),
            b"KAFKA" => Ok(Self::Kafka),
            b"FASTAPI" => Ok(Self::FastApi),
            b"RABITTMQ" => Ok(Self::RabbitMq),
            b"CLI" => Ok(Self::Cli),
            b"UNKNOWN" => Ok(Self::Unknown),
            other => Err(format!(
                "unknown domain_transport: {:?}",
                String::from_utf8_lossy(other)
            )
            .into()),
        }
    }
}

/// Mirrors `riverbase.domain.entity.CommandState` / `command_status` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, AsExpression)]
#[diesel(sql_type = CommandStatusSql)]
pub enum CommandLogStatus {
    /// Success.
    Success,
    /// Created.
    Created,
    /// Pending.
    Pending,
    /// Running.
    Running,
    /// Denied.
    Denied,
    /// Rejected.
    Rejected,
    /// Submitted.
    Submitted,
    /// Applied.
    Applied,
    /// Errored.
    Errored,
    /// Retry1.
    Retry1,
    /// Retry2.
    Retry2,
    /// Retry3.
    Retry3,
    /// Failed.
    Failed,
    /// Canceled.
    Canceled,
}

impl CommandLogStatus {
    /// Borrow as r.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "SUCCESS",
            Self::Created => "CREATED",
            Self::Pending => "PENDING",
            Self::Running => "RUNNING",
            Self::Denied => "DENIED",
            Self::Rejected => "REJECTED",
            Self::Submitted => "SUBMITTED",
            Self::Applied => "APPLIED",
            Self::Errored => "ERRORED",
            Self::Retry1 => "RETRY_1",
            Self::Retry2 => "RETRY_2",
            Self::Retry3 => "RETRY_3",
            Self::Failed => "FAILED",
            Self::Canceled => "CANCELED",
        }
    }
}

/// Status values for `query_log` rows.
///
/// Currently an alias of [`CommandLogStatus`] because the audit schema reuses the
/// `command_status` Postgres enum.
///
/// **DEPRECATED toward split (ARC-05):** introduce a dedicated `query_status` enum /
/// Diesel SQL type once migrations can add it; call sites should treat this as
/// query-specific vocabulary even while it aliases command status.
pub type QueryLogStatus = CommandLogStatus;

/// Mirrors `riverbase.domain.activity.ActivityType` / `activity_msg_type` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, AsExpression)]
#[diesel(sql_type = ActivityMsgTypeSql)]
pub enum ActivityMsgType {
    /// User action.
    UserAction,
    /// App request.
    AppRequest,
    /// System call.
    SystemCall,
}

impl ActivityMsgType {
    /// Borrow as r.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserAction => "USER_ACTION",
            Self::AppRequest => "APP_REQUEST",
            Self::SystemCall => "SYSTEM_CALL",
        }
    }
}

#[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
#[diesel(postgres_type(name = "command_status", schema = "riverbase_audit"))]
/// Command status sql; structure.
pub struct CommandStatusSql;

#[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
#[diesel(postgres_type(name = "activity_msg_type", schema = "riverbase_audit"))]
/// Activity msg type sql; structure.
pub struct ActivityMsgTypeSql;

impl ToSql<CommandStatusSql, Pg> for CommandLogStatus {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        out.write_all(self.as_str().as_bytes())?;
        Ok(serialize::IsNull::No)
    }
}

impl FromSql<CommandStatusSql, Pg> for CommandLogStatus {
    fn from_sql(value: PgValue<'_>) -> deserialize::Result<Self> {
        match value.as_bytes() {
            b"SUCCESS" => Ok(Self::Success),
            b"CREATED" => Ok(Self::Created),
            b"PENDING" => Ok(Self::Pending),
            b"RUNNING" => Ok(Self::Running),
            b"DENIED" => Ok(Self::Denied),
            b"REJECTED" => Ok(Self::Rejected),
            b"SUBMITTED" => Ok(Self::Submitted),
            b"APPLIED" => Ok(Self::Applied),
            b"ERRORED" => Ok(Self::Errored),
            b"RETRY_1" => Ok(Self::Retry1),
            b"RETRY_2" => Ok(Self::Retry2),
            b"RETRY_3" => Ok(Self::Retry3),
            b"FAILED" => Ok(Self::Failed),
            b"CANCELED" => Ok(Self::Canceled),
            other => Err(format!(
                "unknown command_status: {:?}",
                String::from_utf8_lossy(other)
            )
            .into()),
        }
    }
}

impl ToSql<ActivityMsgTypeSql, Pg> for ActivityMsgType {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        out.write_all(self.as_str().as_bytes())?;
        Ok(serialize::IsNull::No)
    }
}

impl FromSql<ActivityMsgTypeSql, Pg> for ActivityMsgType {
    fn from_sql(value: PgValue<'_>) -> deserialize::Result<Self> {
        match value.as_bytes() {
            b"USER_ACTION" => Ok(Self::UserAction),
            b"APP_REQUEST" => Ok(Self::AppRequest),
            b"SYSTEM_CALL" => Ok(Self::SystemCall),
            other => Err(format!(
                "unknown activity_msg_type: {:?}",
                String::from_utf8_lossy(other)
            )
            .into()),
        }
    }
}
