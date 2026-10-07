use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use crate::base::{AuditActor, EngineContext, RiverbaseResult};
use crate::datastore::CommandUnitOfWork;

use super::model::{ActivityLogRecord, LogRowMeta};
use super::types::ActivityMsgType;
use super::util::new_log_id;
use super::DomainLogStore;

/// Parameters for appending an activity log row.
#[derive(Debug, Clone)]
pub struct ActivityEmitParams {
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
    pub msglabel: Option<String>,
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
    /// Creating principal identifier, if known.
    pub creator: Option<Uuid>,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
    /// Tenant data scope (`_tenant`).
    pub tenant: Option<Uuid>,
}

impl ActivityEmitParams {
    /// Construct a new value.
    pub fn new(
        domain: impl Into<String>,
        resource: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        let message = message.into();
        Self {
            source: None,
            domain: domain.into(),
            resource: resource.into(),
            identifier: None,
            domain_sid: None,
            domain_iid: None,
            msglabel: None,
            message,
            msgtype: ActivityMsgType::AppRequest,
            context: None,
            src_cmd: None,
            src_evt: None,
            data: None,
            code: 0,
            creator: None,
            timestamp: Utc::now(),
            tenant: None,
        }
    }

    /// Msgtype.
    pub fn msgtype(mut self, msgtype: ActivityMsgType) -> Self {
        self.msgtype = msgtype;
        self
    }

    /// Machine-readable activity type (stored as `msglabel`).
    pub fn msglabel(mut self, msglabel: impl Into<String>) -> Self {
        self.msglabel = Some(msglabel.into());
        self
    }

    /// Command or event payload.
    pub fn payload(mut self, payload: Value) -> Self {
        self.data = Some(payload);
        self
    }

    /// Execution context identifier.
    pub fn context(mut self, context: Option<Uuid>) -> Self {
        self.context = context;
        self
    }

    /// Src cmd.
    pub fn src_cmd(mut self, src_cmd: Option<Uuid>) -> Self {
        self.src_cmd = src_cmd;
        self
    }

    /// Identifier.
    pub fn identifier(mut self, identifier: Option<Uuid>) -> Self {
        self.identifier = identifier;
        self
    }

    /// Creating principal identifier, if known.
    pub fn creator(mut self, creator: Option<Uuid>) -> Self {
        self.creator = creator;
        self
    }

    /// Timestamp.
    pub fn timestamp(mut self, timestamp: DateTime<Utc>) -> Self {
        self.timestamp = timestamp;
        self
    }

    /// Tenant data scope (`_tenant`).
    pub fn tenant(mut self, tenant: Option<Uuid>) -> Self {
        self.tenant = tenant;
        self
    }

    /// Actor.
    pub fn actor(mut self, actor: &AuditActor) -> Self {
        self.creator = actor.profile_id;
        self.tenant = actor.tenant;
        self
    }

    /// Source.
    pub fn source(mut self, source: Option<String>) -> Self {
        self.source = source;
        self
    }
}

/// Append a single activity log record.
pub async fn append_activity(
    logstore: &DomainLogStore,
    uow: Option<&CommandUnitOfWork>,
    params: ActivityEmitParams,
) -> RiverbaseResult<()> {
    let msglabel = params.msglabel.unwrap_or_else(|| params.message.clone());
    let record = ActivityLogRecord {
        meta: LogRowMeta {
            id: new_log_id(),
            created: params.timestamp,
            creator: params.creator,
        },
        source: params.source,
        domain: params.domain,
        identifier: params.identifier,
        resource: params.resource,
        domain_sid: params.domain_sid,
        domain_iid: params.domain_iid,
        message: params.message,
        msgtype: params.msgtype,
        msglabel,
        context: params.context,
        src_cmd: params.src_cmd,
        src_evt: params.src_evt,
        data: params.data,
        code: params.code,
        tenant: params.tenant,
    };
    logstore.activities.append(uow, record).await
}

/// Emit an activity from a query execution context.
pub async fn emit_query_activity(
    logstore: &DomainLogStore,
    ctx: &EngineContext,
    session_context_id: Uuid,
    query_resource: &str,
    activity_type: impl Into<String>,
    payload: Value,
) -> RiverbaseResult<()> {
    let activity_type = activity_type.into();
    append_activity(
        logstore,
        None,
        ActivityEmitParams::new(ctx.namespace.clone(), query_resource, activity_type.clone())
            .msgtype(ActivityMsgType::AppRequest)
            .payload(payload)
            .context(Some(session_context_id))
            .actor(&ctx.actor)
            .timestamp(Utc::now()),
    )
    .await
}

/// Helper for custom HTTP handlers that need to append activity rows.
#[derive(Clone)]
pub struct ActivityEmitter {
    logstore: DomainLogStore,
    domain: String,
    creator: Option<Uuid>,
    source: Option<String>,
}

impl ActivityEmitter {
    /// Construct a new value.
    pub fn new(logstore: DomainLogStore, domain: impl Into<String>) -> Self {
        Self {
            logstore,
            domain: domain.into(),
            creator: None,
            source: None,
        }
    }

    /// Set creator and return self.
    pub fn with_creator(mut self, creator: Option<Uuid>) -> Self {
        self.creator = creator;
        self
    }

    /// Set source and return self.
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Logstore.
    pub fn logstore(&self) -> &DomainLogStore {
        &self.logstore
    }

    /// Domain namespace.
    pub fn domain(&self) -> &str {
        &self.domain
    }

    /// Emit.
    pub async fn emit(
        &self,
        resource: impl Into<String>,
        activity_type: impl Into<String>,
        payload: Value,
    ) -> RiverbaseResult<()> {
        self.emit_with_options(resource, activity_type, payload, None, None)
            .await
    }

    /// Emit with options.
    pub async fn emit_with_options(
        &self,
        resource: impl Into<String>,
        activity_type: impl Into<String>,
        payload: Value,
        identifier: Option<Uuid>,
        context: Option<Uuid>,
    ) -> RiverbaseResult<()> {
        let activity_type = activity_type.into();
        append_activity(
            &self.logstore,
            None,
            ActivityEmitParams::new(self.domain.clone(), resource, activity_type.clone())
                .msgtype(ActivityMsgType::AppRequest)
                .payload(payload)
                .identifier(identifier)
                .context(context)
                .creator(self.creator)
                .source(self.source.clone()),
        )
        .await
    }
}
