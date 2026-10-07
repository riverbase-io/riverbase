use std::collections::BTreeMap;

use async_trait::async_trait;
use serde_json::Value;

use super::domain::AuditActor;
use super::tenant::{TenantAccess, TenantPolicyResolver};
use crate::RiverbaseResult;

/// Identity used for an in-process command invocation.
///
/// Delegated calls retain the authenticated actor and must pass target-domain policy.
/// Service calls require an explicitly named capability so they cannot silently inherit
/// transport authorization.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum InvocationMode {
    #[default]
    /// Delegated user.
    DelegatedUser,
    /// Service.
    Service,
}

/// Execution context supplied by [`super::domain::Domain`] or engine wrappers on each call.
#[derive(Debug, Clone, Default)]
pub struct EngineContext {
    /// Namespace.
    pub namespace: String,
    /// Human-readable domain title from [`DomainMeta`](crate::domain::DomainMeta).
    pub title: Option<String>,
    /// Correlation id.
    pub correlation_id: Option<String>,
    /// Trace id.
    pub trace_id: Option<String>,
    /// Actor.
    pub actor: AuditActor,
    /// JWT realm roles copied from the authenticated principal (when present).
    pub roles: Vec<String>,
    /// Server-resolved authorization claims (org id, user id, etc.) populated by domain query
    /// wrappers and consumed synchronously by per-resource `scope_policy` blocks.
    ///
    /// JWT token claims are copied under the reserved `jwt.*` prefix and must be read with
    /// [`Self::jwt_claim_str`]. Unprefixed keys are server-derived and must never be set from
    /// client or token input.
    pub claims: BTreeMap<String, Value>,
    /// Client-supplied `Idempotency-Key` header for command deduplication (when present).
    pub idempotency_key: Option<String>,
    /// Reject command payload properties not declared by the typed payload schema.
    pub deny_unknown_fields: bool,
    /// Command that directly caused this invocation.
    pub causation_id: Option<String>,
    /// Immediate parent command for nested command execution.
    pub parent_command_id: Option<String>,
    /// Number of in-process command boundaries crossed from the transport request.
    pub invocation_depth: u16,
    /// Invocation mode.
    pub invocation_mode: InvocationMode,
    /// Required for [`InvocationMode::Service`].
    pub service_capability: Option<String>,
    /// `_tenant IN …` access list (or [`TenantAccess::All`] for `system`).
    pub tenant_access: TenantAccess,
    /// Organization id used by `profile-organization` access and stamp policies.
    pub organization_id: Option<uuid::Uuid>,
    /// Profile id used by the `profile` access and stamp policies.
    pub profile_id: Option<uuid::Uuid>,
    /// Login user id used by the `user` access and stamp policies.
    pub user_id: Option<uuid::Uuid>,
    /// Portal tenant policies. HTTP [`apply`](TenantPolicyResolver::apply) uses these when present.
    pub tenant_policies: Option<TenantPolicyResolver>,
}

impl EngineContext {
    /// Construct a new value.
    pub fn new(namespace: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            title: None,
            correlation_id: None,
            trace_id: None,
            actor: AuditActor::default(),
            roles: Vec::new(),
            claims: BTreeMap::new(),
            idempotency_key: None,
            deny_unknown_fields: false,
            causation_id: None,
            parent_command_id: None,
            invocation_depth: 0,
            invocation_mode: InvocationMode::DelegatedUser,
            service_capability: None,
            tenant_access: TenantAccess::default(),
            organization_id: None,
            profile_id: None,
            user_id: None,
            tenant_policies: None,
        }
    }

    /// Set title and return self.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Title.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Set actor and return self.
    pub fn with_actor(mut self, actor: AuditActor) -> Self {
        self.actor = actor;
        self.profile_id = self.profile_id.or(self.actor.profile_id);
        self.user_id = self.user_id.or(self.actor.user_id);
        if let Some(tenant) = self.actor.tenant {
            self.set_tenant_id(tenant);
        }
        self
    }

    /// Set stamp `_tenant` and, when access is unset, default access to `[stamp]`.
    pub fn set_tenant_id(&mut self, tenant: uuid::Uuid) {
        self.actor = self.actor.clone().with_tenant_id(tenant);
        if self.tenant_access.is_empty() {
            self.tenant_access = TenantAccess::tenants(vec![tenant]);
        }
    }

    /// Set tenant id and return self.
    pub fn with_tenant_id(mut self, tenant: uuid::Uuid) -> Self {
        self.set_tenant_id(tenant);
        self
    }

    /// Replace the tenant access list (or [`TenantAccess::All`]).
    pub fn with_tenant_access(mut self, access: TenantAccess) -> Self {
        self.tenant_access = access;
        self
    }

    /// Access used by filters: empty list falls back to `[stamp]` when a stamp is set.
    pub fn effective_tenant_access(&self) -> TenantAccess {
        match &self.tenant_access {
            TenantAccess::All => TenantAccess::All,
            TenantAccess::Tenants(ids) if ids.is_empty() => self
                .tenant()
                .map(|id| TenantAccess::tenants(vec![id]))
                .unwrap_or_default(),
            other => other.clone(),
        }
    }

    /// Set organization id used by tenant policies.
    pub fn with_organization_id(mut self, organization_id: uuid::Uuid) -> Self {
        self.organization_id = Some(organization_id);
        self.set_claim("organization_id", organization_id.to_string());
        self
    }

    /// Command/query engine context for a domain namespace.
    ///
    /// Does not stamp `_tenant`. Callers must set a tenant with [`Self::with_tenant_id`].
    pub fn for_domain(namespace: impl Into<String>) -> Self {
        Self::new(namespace)
    }

    /// Tenant identifier copied from auth context (`_tenant`).
    pub fn tenant(&self) -> Option<uuid::Uuid> {
        self.actor.tenant
    }

    /// Require a UUID tenant identifier on this engine context (command path).
    pub fn require_tenant(&self) -> crate::base::RiverbaseResult<uuid::Uuid> {
        self.tenant()
            .ok_or_else(|| crate::errors::DOM_045.with_data("missing _tenant"))
    }

    /// Require a UUID tenant identifier on this engine context (query path).
    pub fn require_query_tenant(&self) -> crate::base::RiverbaseResult<uuid::Uuid> {
        self.tenant()
            .ok_or_else(|| crate::errors::QRY_141.with_data("missing _tenant"))
    }

    /// Set profile id and return self.
    pub fn with_profile_id(mut self, profile_id: uuid::Uuid) -> Self {
        self.actor.profile_id = Some(profile_id);
        self.profile_id = Some(profile_id);
        self
    }

    /// Set login user id used by the `user` tenant policies.
    pub fn with_user_id(mut self, user_id: uuid::Uuid) -> Self {
        self.actor.user_id = Some(user_id);
        self.user_id = Some(user_id);
        self
    }

    /// Set a resolved authorization claim (server-side only).
    pub fn with_claim(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.claims.insert(key.into(), value.into());
        self
    }

    /// Insert a resolved authorization claim in place (server-side only).
    ///
    /// Keys under the reserved `jwt.` prefix are ignored so a token cannot overwrite
    /// server-derived values by colliding on the same name.
    pub fn set_claim(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        let key = key.into();
        if key.starts_with("jwt.") {
            return;
        }
        self.claims.insert(key, value.into());
    }

    /// Copy raw JWT claims under the reserved `jwt.*` prefix.
    pub fn set_jwt_claims(&mut self, claims: &serde_json::Map<String, Value>) {
        for (key, value) in claims {
            self.claims.insert(format!("jwt.{key}"), value.clone());
        }
    }

    /// Look up a resolved claim value.
    pub fn claim(&self, key: &str) -> Option<&Value> {
        self.claims.get(key)
    }

    /// Look up a resolved claim as a string slice.
    pub fn claim_str(&self, key: &str) -> Option<&str> {
        self.claims.get(key).and_then(Value::as_str)
    }

    /// Look up a raw JWT claim copied under `jwt.*`.
    pub fn jwt_claim(&self, key: &str) -> Option<&Value> {
        self.claims.get(&format!("jwt.{key}"))
    }

    /// Look up a raw JWT claim as a string slice.
    pub fn jwt_claim_str(&self, key: &str) -> Option<&str> {
        self.jwt_claim(key).and_then(Value::as_str)
    }

    /// Set correlation id and return self.
    pub fn with_correlation_id(mut self, value: impl Into<String>) -> Self {
        self.correlation_id = Some(value.into());
        self
    }

    /// Set trace id and return self.
    pub fn with_trace_id(mut self, value: impl Into<String>) -> Self {
        self.trace_id = Some(value.into());
        self
    }

    /// Set idempotency key and return self.
    pub fn with_idempotency_key(mut self, value: impl Into<String>) -> Self {
        self.idempotency_key = Some(value.into());
        self
    }

    /// Set deny unknown fields and return self.
    pub fn with_deny_unknown_fields(mut self, enabled: bool) -> Self {
        self.deny_unknown_fields = enabled;
        self
    }

    /// Set causation id and return self.
    pub fn with_causation_id(mut self, value: impl Into<String>) -> Self {
        self.causation_id = Some(value.into());
        self
    }

    /// Set parent command id and return self.
    pub fn with_parent_command_id(mut self, value: impl Into<String>) -> Self {
        self.parent_command_id = Some(value.into());
        self
    }

    /// Delegated.
    pub fn delegated(mut self) -> Self {
        self.invocation_mode = InvocationMode::DelegatedUser;
        self.service_capability = None;
        self
    }

    /// Borrow as rvice.
    pub fn as_service(mut self, capability: impl Into<String>) -> Self {
        self.invocation_mode = InvocationMode::Service;
        self.service_capability = Some(capability.into());
        self
    }

    /// Namespace.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Ensure namespace.
    pub fn ensure_namespace(&self, expected: &str) -> RiverbaseResult<()> {
        if expected != self.namespace {
            return Err(crate::errors::C00_006.with_data(format!(
                "namespace mismatch: expected {expected}, got {}",
                self.namespace
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Engine Kind enumeration.
pub enum EngineKind {
    /// Command.
    Command,
    /// Query.
    Query,
    /// Service.
    Service,
}

/// Common surface for command and query engine actors.
#[async_trait]
pub trait Engine: Send + Sync {
    /// Kind.
    fn kind(&self) -> EngineKind;

    /// Local item keys (command keys or query resource names).
    async fn items(&self) -> RiverbaseResult<Vec<String>>;
}

#[cfg(test)]
mod tests {
    use super::EngineContext;
    use serde_json::json;

    #[test]
    fn jwt_claims_cannot_overwrite_server_derived_organization_id() {
        let mut ctx = EngineContext::new("sourcing");
        ctx.set_claim("organization_id", "server-org");
        ctx.set_jwt_claims(
            json!({ "organization_id": "token-org" })
                .as_object()
                .unwrap(),
        );
        assert_eq!(ctx.claim_str("organization_id"), Some("server-org"));
        assert_eq!(ctx.jwt_claim_str("organization_id"), Some("token-org"));
    }

    #[test]
    fn set_claim_ignores_jwt_prefix() {
        let mut ctx = EngineContext::new("sourcing");
        ctx.set_claim("jwt.organization_id", "forged");
        assert!(ctx.jwt_claim_str("organization_id").is_none());
    }

    #[test]
    fn require_tenant_fails_closed_without_uuid() {
        let ctx = EngineContext::new("riverbase.todo");
        let err = ctx.require_tenant().expect_err("DOM-045");
        assert_eq!(err.errcode.as_str(), "DOM-045");
        let err = ctx.require_query_tenant().expect_err("QRY-141");
        assert_eq!(err.errcode.as_str(), "QRY-141");
    }

    #[test]
    fn with_tenant_id_satisfies_require_tenant() {
        let tenant = uuid::Uuid::new_v4();
        let ctx = EngineContext::new("riverbase.todo").with_tenant_id(tenant);
        assert_eq!(ctx.require_tenant().expect("tenant"), tenant);
        assert_eq!(ctx.require_query_tenant().expect("tenant"), tenant);
        assert_eq!(
            ctx.effective_tenant_access(),
            super::TenantAccess::tenants(vec![tenant])
        );
    }

    #[test]
    fn explicit_all_access_is_not_replaced_by_stamp() {
        let tenant = uuid::Uuid::new_v4();
        let ctx = EngineContext::new("riverbase.todo")
            .with_tenant_access(super::TenantAccess::All)
            .with_tenant_id(tenant);
        assert_eq!(ctx.tenant(), Some(tenant));
        assert!(ctx.effective_tenant_access().is_all());
    }
}
