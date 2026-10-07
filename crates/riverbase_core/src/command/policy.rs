use async_trait::async_trait;
use serde_json::Value;

use super::CommandTarget;
use crate::base::{EngineContext, RiverbaseResult};

/// Application-layer command authorization/invariant precondition.
///
/// Policies run for every engine invocation, including HTTP, workers, tests, and delegated
/// in-process commands.
#[async_trait]
pub trait CommandPolicy: Send + Sync {
    /// Stable name for startup reports and tests ([SEC-08]).
    fn name(&self) -> &'static str {
        "unnamed"
    }

    /// Authorize.
    async fn authorize(
        &self,
        ctx: &EngineContext,
        command: &str,
        payload: &Value,
        target: &CommandTarget,
    ) -> RiverbaseResult<()>;
}

/// Framework policy installed when the engine is spawned with an empty policy vector.
///
/// HTTP Casbin and [`crate::domain::CommandActivityGate`] remain the authorization sources.
/// This type exists so the empty-vector path is gone and every engine has a named policy.
pub struct DefaultCommandPolicy;

impl DefaultCommandPolicy {
    /// Canonical name.
    pub const NAME: &'static str = "riverbase.default";
}

#[async_trait]
impl CommandPolicy for DefaultCommandPolicy {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    async fn authorize(
        &self,
        _ctx: &EngineContext,
        _command: &str,
        _payload: &Value,
        _target: &CommandTarget,
    ) -> RiverbaseResult<()> {
        Ok(())
    }
}
