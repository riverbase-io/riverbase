use chrono::Utc;
use serde_json::Value;
use uuid::Uuid;

use crate::base::{EngineContext, RiverbaseResult};
use crate::logstore::{
    append_activity, emit_query_activity, ActivityEmitParams, ActivityMsgType, DomainLogStore,
};

/// Per-request query execution context for optional activity emission.
#[derive(Clone)]
pub struct QuerySession {
    logstore: DomainLogStore,
    ctx: EngineContext,
    context_id: Uuid,
    query_resource: String,
}

impl QuerySession {
    /// Construct a new value.
    pub fn new(
        logstore: DomainLogStore,
        ctx: EngineContext,
        context_id: Uuid,
        query_resource: impl Into<String>,
    ) -> Self {
        Self {
            logstore,
            ctx,
            context_id,
            query_resource: query_resource.into(),
        }
    }

    /// Context id.
    pub fn context_id(&self) -> Uuid {
        self.context_id
    }

    /// Engine context.
    pub fn engine_context(&self) -> &EngineContext {
        &self.ctx
    }

    /// Query resource.
    pub fn query_resource(&self) -> &str {
        &self.query_resource
    }

    /// Emit activity.
    pub async fn emit_activity(
        &self,
        activity_type: impl Into<String>,
        payload: Value,
    ) -> RiverbaseResult<()> {
        emit_query_activity(
            &self.logstore,
            &self.ctx,
            self.context_id,
            &self.query_resource,
            activity_type,
            payload,
        )
        .await
    }

    /// Emit activity for resource.
    pub async fn emit_activity_for_resource(
        &self,
        resource: impl Into<String>,
        activity_type: impl Into<String>,
        payload: Value,
    ) -> RiverbaseResult<()> {
        let activity_type = activity_type.into();
        append_activity(
            &self.logstore,
            None,
            ActivityEmitParams::new(self.ctx.namespace.clone(), resource, activity_type)
                .msgtype(ActivityMsgType::AppRequest)
                .payload(payload)
                .context(Some(self.context_id))
                .actor(&self.ctx.actor)
                .timestamp(Utc::now()),
        )
        .await
    }
}
