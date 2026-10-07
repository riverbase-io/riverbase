//! Casbin-backed [`CommandActivityGate`] for `CommandInvoker` ([SEC-08], [SEC-11]).

use std::sync::Arc;

use async_trait::async_trait;

use crate::base::{EngineContext, RiverbaseResult, InvocationMode};
use crate::casbin::{
    ActivityAuthorizer, ActivityRequest, CasbinActivityAuthorizer, DomainActivity,
};
use crate::command::{CommandAuthz, CommandMeta};
use crate::domain::CommandActivityGate;
use crate::web::route_auth::RouteAuthState;
use serde_json::json;

/// Re-evaluates `{cmdkey}.execute` against the target domain's Casbin identity.
pub struct CasbinCommandGate {
    authorizer: Arc<CasbinActivityAuthorizer>,
    route_auth: RouteAuthState,
    known_roles: Arc<Vec<String>>,
}

impl CasbinCommandGate {
    /// Construct a new value.
    pub fn new(
        authorizer: Arc<CasbinActivityAuthorizer>,
        route_auth: RouteAuthState,
        known_roles: Arc<Vec<String>>,
    ) -> Self {
        Self {
            authorizer,
            route_auth,
            known_roles,
        }
    }
}

fn capability_grants(capability: &str, namespace: &str, cmdkey: &str) -> bool {
    capability == "*"
        || capability == cmdkey
        || capability == format!("{namespace}.{cmdkey}")
        || capability == format!("{namespace}.{cmdkey}.execute")
        || capability == format!("{cmdkey}.execute")
}

fn subject_from_context(ctx: &EngineContext, known_roles: &[String]) -> String {
    known_roles
        .iter()
        .find(|role| ctx.roles.iter().any(|held| held == *role))
        .cloned()
        .or_else(|| ctx.roles.first().cloned())
        .or_else(|| ctx.actor.profile_id.map(|id| id.to_string()))
        .unwrap_or_default()
}

#[async_trait]
impl CommandActivityGate for CasbinCommandGate {
    async fn authorize(
        &self,
        ctx: &EngineContext,
        namespace: &str,
        cmdkey: &str,
        meta: Option<&CommandMeta>,
    ) -> RiverbaseResult<()> {
        if matches!(meta.map(|m| &m.authz), Some(CommandAuthz::Public { .. })) {
            return Ok(());
        }
        if ctx.invocation_mode == InvocationMode::Service {
            if let Some(capability) = &ctx.service_capability {
                if capability_grants(capability, namespace, cmdkey) {
                    return Ok(());
                }
            }
        }
        let identity = self.route_auth.policy_identity(namespace).ok_or_else(|| {
            crate::errors::CAS_011.with_data(json!({
                "namespace": namespace,
                "command": cmdkey,
                "detail": "unregistered policy identity",
            }))
        })?;
        let resource = meta
            .and_then(|m| m.resources.first())
            .map(String::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or("*");
        let activity = DomainActivity::new(identity, format!("{cmdkey}.execute"))
            .with_resource(resource)
            .with_object_id("*");
        let subject = subject_from_context(ctx, &self.known_roles);
        if subject.is_empty() {
            return Err(riverbase_core::errors::CAS_010.with_data(json!({
                "namespace": namespace,
                "command": cmdkey,
                "detail": "missing principal",
            })));
        }
        self.authorizer
            .enforce(&ActivityRequest::new(subject, activity))
            .await
    }
}
