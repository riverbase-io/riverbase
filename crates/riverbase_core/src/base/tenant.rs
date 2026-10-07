//! Tenant access lists (query/command filters) and stamp UUIDs (row `_tenant`).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use super::engine::EngineContext;
use super::error::RiverbaseResult;
use crate::datastore::dsl::Expr;

/// Portal `[riverbase] tenant_access_policy` / `RIVERBASE_TENANT_ACCESS_POLICY`.
pub const DEFAULT_TENANT_ACCESS_POLICY: &str = "profile-organization";
/// Portal `[riverbase] tenant_stamp_policy` / `RIVERBASE_TENANT_STAMP_POLICY`.
pub const DEFAULT_TENANT_STAMP_POLICY: &str = "profile-organization";

/// Builtin access policy names. Apps may register additional names.
pub const ACCESS_PROFILE_ORGANIZATION: &str = "profile-organization";
/// Access list is `[profile.id]`.
pub const ACCESS_PROFILE: &str = "profile";
/// Access list is `[login user id]`.
pub const ACCESS_USER: &str = "user";
/// Access list is the mapped tenant for the Riverbase namespace.
pub const ACCESS_DOMAIN_TENANT: &str = "domain-tenant";
/// Omit the `_tenant` predicate (coordinator / operator portals).
pub const ACCESS_SYSTEM: &str = "system";

/// Builtin stamp policy names. There is no `system` stamp — a row has one `_tenant`.
pub const STAMP_PROFILE_ORGANIZATION: &str = "profile-organization";
/// Stamp `_tenant` with `profile.id`.
pub const STAMP_PROFILE: &str = "profile";
/// Stamp `_tenant` with the login user id.
pub const STAMP_USER: &str = "user";
/// Stamp `_tenant` from the domain→tenant mapping.
pub const STAMP_DOMAIN_TENANT: &str = "domain-tenant";

/// Resolved access used by `_tenant IN …` filters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TenantAccess {
    /// Fail-closed or explicit list. Empty matches no rows.
    Tenants(Vec<Uuid>),
    /// `system` — omit the `_tenant` predicate. Do not enumerate every UUID.
    All,
}

impl Default for TenantAccess {
    fn default() -> Self {
        Self::Tenants(Vec::new())
    }
}

impl TenantAccess {
    /// Explicit tenant list (empty is fail-closed).
    pub fn tenants(ids: impl Into<Vec<Uuid>>) -> Self {
        Self::Tenants(ids.into())
    }

    /// Whether this is unrestricted `system` access.
    pub fn is_all(&self) -> bool {
        matches!(self, Self::All)
    }

    /// Whether this is an empty list (matches no rows).
    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Tenants(ids) if ids.is_empty())
    }

    /// Tenant UUIDs when this is a list policy.
    pub fn as_tenants(&self) -> Option<&[Uuid]> {
        match self {
            Self::Tenants(ids) => Some(ids),
            Self::All => None,
        }
    }

    /// Whether `tenant` is in the access list (or access is `All`).
    pub fn allows(&self, tenant: Uuid) -> bool {
        match self {
            Self::All => true,
            Self::Tenants(ids) => ids.contains(&tenant),
        }
    }

    /// Whether a persisted row's `_tenant` is visible under this access list.
    pub fn allows_row(&self, row: &Value) -> bool {
        match self {
            Self::All => true,
            Self::Tenants(ids) => row_tenant_id(row).is_some_and(|id| ids.contains(&id)),
        }
    }

    /// `_tenant IN list`, or `None` when the predicate should be omitted.
    pub fn filter_expr(&self) -> Option<Expr> {
        match self {
            Self::All => None,
            Self::Tenants(ids) if ids.is_empty() => None,
            Self::Tenants(ids) => Some(Expr::in_list(
                "_tenant",
                ids.iter()
                    .map(|id| Value::String(id.to_string()))
                    .collect::<Vec<_>>(),
            )),
        }
    }
}

/// Write `_tenant` for a new row. Any payload value is replaced.
pub fn apply_tenant_stamp(data: &mut Value, resolved: Uuid) {
    if let Some(obj) = data.as_object_mut() {
        obj.insert("_tenant".to_string(), json!(resolved.to_string()));
    }
}

/// Carry the stored `_tenant` onto an update. Any payload value is replaced.
///
/// The stored value is copied verbatim, including a non-UUID string, so a later
/// serialize-the-whole-row write does not null the column. Absent on the stored
/// row means absent on the write — never invent one from the actor.
pub fn carry_row_tenant(data: &mut Value, current: &Value) {
    let Some(obj) = data.as_object_mut() else {
        return;
    };
    match current.get("_tenant") {
        Some(tenant) => {
            obj.insert("_tenant".to_string(), tenant.clone());
        }
        None => {
            obj.remove("_tenant");
        }
    }
}

/// Whether a transfer should write, or the destination already matches the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenantTransfer {
    /// Destination equals the stored tenant. Do not write.
    Unchanged,
    /// Write `destination` as the new `_tenant`.
    Move(Uuid),
}

/// Allow a transfer only when `actor` is the row's current tenant.
///
/// A row with no parseable `_tenant`, and an actor who is not that tenant,
/// both fail with `DOM-050`.
pub fn authorize_tenant_transfer(
    actor: Uuid,
    current: &Value,
    destination: Uuid,
) -> RiverbaseResult<TenantTransfer> {
    let Some(source) = row_tenant_id(current) else {
        return Err(crate::errors::DOM_050.with_data("row has no parseable _tenant"));
    };
    if actor != source {
        return Err(crate::errors::DOM_050.with_data(json!({
            "source": source.to_string(),
        })));
    }
    if destination == source {
        return Ok(TenantTransfer::Unchanged);
    }
    Ok(TenantTransfer::Move(destination))
}

/// Parse `_tenant` from a persisted JSON row.
pub fn row_tenant_id(row: &Value) -> Option<Uuid> {
    row.get("_tenant")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
}

/// Query vs command plus the resource the filter applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenantAccessKind {
    /// Query / read path.
    Query,
    /// Command / write path.
    Command,
}

/// Inputs for a named access-policy function.
#[derive(Debug, Clone)]
pub struct TenantAccessContext<'a> {
    /// Query vs command.
    pub kind: TenantAccessKind,
    /// Riverbase domain namespace.
    pub namespace: &'a str,
    /// Query resource or command resource name.
    pub resource: &'a str,
}

/// App-registered access policy: `(ctx, access) -> tenant list`.
pub type TenantAccessFn =
    Arc<dyn Fn(&EngineContext, &TenantAccessContext<'_>) -> RiverbaseResult<Vec<Uuid>> + Send + Sync>;

/// Lookup Riverbase namespace → tenant UUID (`rfx.idm.domain_tenant`).
pub trait DomainTenantLookup: Send + Sync {
    /// Tenant UUID for a Riverbase namespace (`exp.catalog`, `rfx.idm`, …).
    fn tenant_for_domain(&self, namespace: &str) -> Option<Uuid>;
}

/// Resolves portal access + stamp policies. Cheap to clone (`Arc` internals).
#[derive(Clone)]
pub struct TenantPolicyResolver {
    inner: Arc<TenantPolicyInner>,
}

struct TenantPolicyInner {
    access_policy: String,
    stamp_policy: String,
    extra_access: RwLock<HashMap<String, TenantAccessFn>>,
    domain_lookup: RwLock<Option<Arc<dyn DomainTenantLookup>>>,
}

impl std::fmt::Debug for TenantPolicyResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TenantPolicyResolver")
            .field("access_policy", &self.inner.access_policy)
            .field("stamp_policy", &self.inner.stamp_policy)
            .finish()
    }
}

impl TenantPolicyResolver {
    /// Construct a resolver for the named portal policies.
    pub fn new(access_policy: impl Into<String>, stamp_policy: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(TenantPolicyInner {
                access_policy: access_policy.into(),
                stamp_policy: stamp_policy.into(),
                extra_access: RwLock::new(HashMap::new()),
                domain_lookup: RwLock::new(None),
            }),
        }
    }

    /// Default `profile-organization` access and stamp.
    pub fn from_defaults() -> Self {
        Self::new(DEFAULT_TENANT_ACCESS_POLICY, DEFAULT_TENANT_STAMP_POLICY)
    }

    /// Configured access policy name.
    pub fn access_policy(&self) -> &str {
        &self.inner.access_policy
    }

    /// Configured stamp policy name.
    pub fn stamp_policy(&self) -> &str {
        &self.inner.stamp_policy
    }

    /// Register an extra named access policy (in addition to builtins).
    pub fn register_tenant_access_policy(&self, name: impl Into<String>, func: TenantAccessFn) {
        self.inner
            .extra_access
            .write()
            .expect("tenant access policy map")
            .insert(name.into(), func);
    }

    /// Install the namespace → tenant lookup required by `domain-tenant`.
    pub fn set_domain_tenant_lookup(&self, lookup: Arc<dyn DomainTenantLookup>) {
        *self
            .inner
            .domain_lookup
            .write()
            .expect("domain tenant lookup") = Some(lookup);
    }

    /// Current domain → tenant lookup, if registered.
    pub fn domain_lookup(&self) -> Option<Arc<dyn DomainTenantLookup>> {
        self.inner
            .domain_lookup
            .read()
            .expect("domain tenant lookup")
            .clone()
    }

    /// Fail startup on unknown policy names or `domain-tenant` without a lookup.
    pub fn validate_startup(&self) -> RiverbaseResult<()> {
        let access = self.access_policy();
        if !is_builtin_access(access) && !self.has_extra_access(access) {
            return Err(unknown_access_policy(access));
        }
        if access == ACCESS_DOMAIN_TENANT && self.domain_lookup().is_none() {
            return Err(missing_domain_lookup("tenant_access_policy"));
        }
        let stamp = self.stamp_policy();
        if !is_builtin_stamp(stamp) {
            return Err(unknown_stamp_policy(stamp));
        }
        if stamp == STAMP_DOMAIN_TENANT && self.domain_lookup().is_none() {
            return Err(missing_domain_lookup("tenant_stamp_policy"));
        }
        Ok(())
    }

    fn has_extra_access(&self, name: &str) -> bool {
        self.inner
            .extra_access
            .read()
            .expect("tenant access policy map")
            .contains_key(name)
    }

    /// Resolve the UUID written to `_tenant` when a row is created.
    pub fn resolve_stamp(&self, ctx: &EngineContext) -> RiverbaseResult<Uuid> {
        resolve_builtin_stamp(self.stamp_policy(), ctx, self.domain_lookup().as_deref())
    }

    /// Resolve the access list (or [`TenantAccess::All`]) for filters.
    pub fn resolve_access(
        &self,
        ctx: &EngineContext,
        access: &TenantAccessContext<'_>,
    ) -> RiverbaseResult<TenantAccess> {
        let name = self.access_policy();
        if name == ACCESS_SYSTEM {
            return Ok(TenantAccess::All);
        }
        if is_builtin_access(name) {
            return Ok(TenantAccess::Tenants(resolve_builtin_access_list(
                name,
                ctx,
                self.domain_lookup().as_deref(),
            )?));
        }
        let func = self
            .inner
            .extra_access
            .read()
            .expect("tenant access policy map")
            .get(name)
            .cloned()
            .ok_or_else(|| unknown_access_policy(name))?;
        Ok(TenantAccess::Tenants(func(ctx, access)?))
    }

    /// Stamp `_tenant` and replace `tenant_access` from the portal policies.
    pub fn apply(
        &self,
        ctx: &mut EngineContext,
        access: &TenantAccessContext<'_>,
    ) -> RiverbaseResult<()> {
        let stamp = self.resolve_stamp(ctx)?;
        ctx.set_tenant_id(stamp);
        ctx.tenant_access = self.resolve_access(ctx, access)?;
        Ok(())
    }
}

fn is_builtin_access(name: &str) -> bool {
    matches!(
        name,
        ACCESS_PROFILE_ORGANIZATION
            | ACCESS_PROFILE
            | ACCESS_USER
            | ACCESS_DOMAIN_TENANT
            | ACCESS_SYSTEM
    )
}

fn is_builtin_stamp(name: &str) -> bool {
    matches!(
        name,
        STAMP_PROFILE_ORGANIZATION | STAMP_PROFILE | STAMP_USER | STAMP_DOMAIN_TENANT
    )
}

fn resolve_builtin_stamp(
    name: &str,
    ctx: &EngineContext,
    lookup: Option<&dyn DomainTenantLookup>,
) -> RiverbaseResult<Uuid> {
    match name {
        STAMP_PROFILE_ORGANIZATION => ctx
            .organization_id
            .ok_or_else(|| missing_stamp_source(name)),
        STAMP_PROFILE => ctx.profile_id.ok_or_else(|| missing_stamp_source(name)),
        STAMP_USER => ctx.user_id.ok_or_else(|| missing_stamp_source(name)),
        STAMP_DOMAIN_TENANT => lookup
            .and_then(|lookup| lookup.tenant_for_domain(ctx.namespace()))
            .ok_or_else(|| missing_domain_mapping(ctx.namespace())),
        other => Err(unknown_stamp_policy(other)),
    }
}

fn resolve_builtin_access_list(
    name: &str,
    ctx: &EngineContext,
    lookup: Option<&dyn DomainTenantLookup>,
) -> RiverbaseResult<Vec<Uuid>> {
    Ok(match name {
        ACCESS_PROFILE_ORGANIZATION => ctx.organization_id.into_iter().collect(),
        ACCESS_PROFILE => ctx.profile_id.into_iter().collect(),
        ACCESS_USER => ctx.user_id.into_iter().collect(),
        ACCESS_DOMAIN_TENANT => lookup
            .and_then(|lookup| lookup.tenant_for_domain(ctx.namespace()))
            .into_iter()
            .collect(),
        ACCESS_SYSTEM => unreachable!("system is TenantAccess::All"),
        other => return Err(unknown_access_policy(other)),
    })
}

fn unknown_access_policy(name: &str) -> crate::base::RiverbaseError {
    crate::errors::CFG_160.with_data(json!({
        "tenant_access_policy": name,
        "builtins": [
            ACCESS_PROFILE_ORGANIZATION,
            ACCESS_PROFILE,
            ACCESS_USER,
            ACCESS_DOMAIN_TENANT,
            ACCESS_SYSTEM,
        ],
    }))
}

fn unknown_stamp_policy(name: &str) -> crate::base::RiverbaseError {
    crate::errors::CFG_161.with_data(json!({
        "tenant_stamp_policy": name,
        "builtins": [
            STAMP_PROFILE_ORGANIZATION,
            STAMP_PROFILE,
            STAMP_USER,
            STAMP_DOMAIN_TENANT,
        ],
    }))
}

fn missing_domain_lookup(field: &str) -> crate::base::RiverbaseError {
    crate::errors::CFG_162.with_data(json!({ "field": field, "policy": ACCESS_DOMAIN_TENANT }))
}

fn missing_stamp_source(policy: &str) -> crate::base::RiverbaseError {
    crate::errors::DOM_047.with_data(json!({ "tenant_stamp_policy": policy }))
}

fn missing_domain_mapping(namespace: &str) -> crate::base::RiverbaseError {
    crate::errors::DOM_048
        .with_data(json!({ "namespace": namespace, "tenant_stamp_policy": STAMP_DOMAIN_TENANT }))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MapLookup(HashMap<String, Uuid>);

    impl DomainTenantLookup for MapLookup {
        fn tenant_for_domain(&self, namespace: &str) -> Option<Uuid> {
            self.0.get(namespace).copied()
        }
    }

    fn ctx_with(org: Option<Uuid>, profile: Option<Uuid>, user: Option<Uuid>) -> EngineContext {
        let mut ctx = EngineContext::new("exp.catalog");
        ctx.organization_id = org;
        ctx.profile_id = profile;
        ctx.user_id = user;
        ctx
    }

    fn access_ctx() -> TenantAccessContext<'static> {
        TenantAccessContext {
            kind: TenantAccessKind::Query,
            namespace: "exp.catalog",
            resource: "poi",
        }
    }

    #[test]
    fn profile_organization_stamp_and_access() {
        let org = Uuid::new_v4();
        let resolver = TenantPolicyResolver::from_defaults();
        let ctx = ctx_with(Some(org), None, None);
        assert_eq!(resolver.resolve_stamp(&ctx).unwrap(), org);
        assert_eq!(
            resolver.resolve_access(&ctx, &access_ctx()).unwrap(),
            TenantAccess::tenants(vec![org])
        );
    }

    #[test]
    fn missing_org_is_fail_closed_access_and_stamp_error() {
        let resolver = TenantPolicyResolver::from_defaults();
        let ctx = ctx_with(None, None, None);
        assert!(resolver
            .resolve_access(&ctx, &access_ctx())
            .unwrap()
            .is_empty());
        let err = resolver.resolve_stamp(&ctx).expect_err("stamp");
        assert_eq!(err.errcode.as_str(), "DOM-047");
    }

    #[test]
    fn profile_and_user_builtins() {
        let profile = Uuid::new_v4();
        let user = Uuid::new_v4();
        let resolver = TenantPolicyResolver::new(ACCESS_PROFILE, STAMP_USER);
        let ctx = ctx_with(None, Some(profile), Some(user));
        assert_eq!(
            resolver.resolve_access(&ctx, &access_ctx()).unwrap(),
            TenantAccess::tenants(vec![profile])
        );
        assert_eq!(resolver.resolve_stamp(&ctx).unwrap(), user);
    }

    #[test]
    fn system_access_is_all_and_stamp_stays_org() {
        let org = Uuid::new_v4();
        let resolver = TenantPolicyResolver::new(ACCESS_SYSTEM, STAMP_PROFILE_ORGANIZATION);
        let ctx = ctx_with(Some(org), None, None);
        assert_eq!(
            resolver.resolve_access(&ctx, &access_ctx()).unwrap(),
            TenantAccess::All
        );
        assert_eq!(resolver.resolve_stamp(&ctx).unwrap(), org);
    }

    #[test]
    fn domain_tenant_uses_lookup() {
        let tenant = Uuid::new_v4();
        let resolver = TenantPolicyResolver::new(ACCESS_DOMAIN_TENANT, STAMP_DOMAIN_TENANT);
        resolver.set_domain_tenant_lookup(Arc::new(MapLookup(HashMap::from([(
            "exp.catalog".into(),
            tenant,
        )]))));
        let ctx = EngineContext::new("exp.catalog");
        assert_eq!(resolver.resolve_stamp(&ctx).unwrap(), tenant);
        assert_eq!(
            resolver.resolve_access(&ctx, &access_ctx()).unwrap(),
            TenantAccess::tenants(vec![tenant])
        );
    }

    #[test]
    fn missing_domain_mapping_is_fail_closed() {
        let resolver = TenantPolicyResolver::new(ACCESS_DOMAIN_TENANT, STAMP_DOMAIN_TENANT);
        resolver.set_domain_tenant_lookup(Arc::new(MapLookup(HashMap::new())));
        let ctx = EngineContext::new("exp.catalog");
        assert!(resolver
            .resolve_access(&ctx, &access_ctx())
            .unwrap()
            .is_empty());
        assert_eq!(
            resolver
                .resolve_stamp(&ctx)
                .expect_err("stamp")
                .errcode
                .as_str(),
            "DOM-048"
        );
    }

    #[test]
    fn domain_tenant_without_lookup_fails_startup() {
        let resolver = TenantPolicyResolver::new(ACCESS_DOMAIN_TENANT, STAMP_PROFILE);
        let err = resolver.validate_startup().expect_err("lookup required");
        assert_eq!(err.errcode.as_str(), "CFG-162");
    }

    #[test]
    fn unknown_policy_names_fail_startup() {
        let err = TenantPolicyResolver::new("managed-orgs", STAMP_PROFILE)
            .validate_startup()
            .expect_err("unknown access");
        assert_eq!(err.errcode.as_str(), "CFG-160");
        let err = TenantPolicyResolver::new(ACCESS_PROFILE, "system")
            .validate_startup()
            .expect_err("unknown stamp");
        assert_eq!(err.errcode.as_str(), "CFG-161");
    }

    #[test]
    fn registered_access_policy_is_allowed() {
        let extra = Uuid::new_v4();
        let resolver = TenantPolicyResolver::new("managed-orgs", STAMP_PROFILE);
        resolver
            .register_tenant_access_policy("managed-orgs", Arc::new(move |_, _| Ok(vec![extra])));
        resolver.validate_startup().unwrap();
        let ctx = ctx_with(None, Some(Uuid::new_v4()), None);
        assert_eq!(
            resolver.resolve_access(&ctx, &access_ctx()).unwrap(),
            TenantAccess::tenants(vec![extra])
        );
    }

    #[test]
    fn empty_access_filter_expr_is_none_never_all() {
        assert!(TenantAccess::tenants(vec![]).filter_expr().is_none());
        assert!(!TenantAccess::tenants(vec![]).is_all());
        let id = Uuid::new_v4();
        assert!(TenantAccess::tenants(vec![id]).filter_expr().is_some());
        assert!(TenantAccess::All.filter_expr().is_none());
    }

    #[test]
    fn apply_overwrites_access_after_stamp() {
        let org = Uuid::new_v4();
        let other = Uuid::new_v4();
        let resolver = TenantPolicyResolver::new(ACCESS_SYSTEM, STAMP_PROFILE_ORGANIZATION);
        let mut ctx = ctx_with(Some(org), None, None);
        ctx.set_tenant_id(other);
        resolver.apply(&mut ctx, &access_ctx()).unwrap();
        assert_eq!(ctx.tenant(), Some(org));
        assert_eq!(ctx.tenant_access, TenantAccess::All);
    }

    #[test]
    fn preserve_row_tenant_is_rejected() {
        let resolver = TenantPolicyResolver::new(ACCESS_SYSTEM, "preserve-row-tenant");
        let err = resolver.validate_startup().expect_err("startup");
        assert_eq!(err.errcode.as_str(), "CFG-161");
    }

    #[test]
    fn apply_tenant_stamp_replaces_payload_tenant() {
        let actor = Uuid::new_v4();
        let attacker = Uuid::new_v4();
        let mut insert = json!({ "_tenant": attacker.to_string(), "name": "created" });
        apply_tenant_stamp(&mut insert, actor);
        assert_eq!(insert["_tenant"], json!(actor.to_string()));
    }

    #[test]
    fn carry_row_tenant_keeps_stored_value() {
        let owner = Uuid::new_v4();
        let attacker = Uuid::new_v4();
        let current = json!({ "_tenant": owner.to_string(), "name": "kept" });
        let mut update = json!({ "_tenant": attacker.to_string(), "name": "edited" });
        carry_row_tenant(&mut update, &current);
        assert_eq!(update["_tenant"], json!(owner.to_string()));
        assert_eq!(update["name"], json!("edited"));

        let garbage = json!({ "_tenant": "not-a-uuid" });
        let mut patched = json!({ "_tenant": attacker.to_string() });
        carry_row_tenant(&mut patched, &garbage);
        assert_eq!(patched["_tenant"], json!("not-a-uuid"));

        let blank = json!({ "name": "legacy" });
        let mut supplied = json!({ "_tenant": attacker.to_string(), "name": "edited" });
        carry_row_tenant(&mut supplied, &blank);
        assert!(supplied.get("_tenant").is_none());
    }

    #[test]
    fn authorize_tenant_transfer_is_source_only() {
        let owner = Uuid::new_v4();
        let other = Uuid::new_v4();
        let destination = Uuid::new_v4();
        let current = json!({ "_tenant": owner.to_string() });

        assert_eq!(
            authorize_tenant_transfer(owner, &current, destination).unwrap(),
            TenantTransfer::Move(destination)
        );
        assert_eq!(
            authorize_tenant_transfer(owner, &current, owner).unwrap(),
            TenantTransfer::Unchanged
        );

        let denied = authorize_tenant_transfer(other, &current, destination).expect_err("actor");
        assert_eq!(denied.errcode.as_str(), "DOM-050");

        let blank = json!({ "name": "legacy" });
        let unparsed = authorize_tenant_transfer(owner, &blank, destination).expect_err("tenant");
        assert_eq!(unparsed.errcode.as_str(), "DOM-050");
    }
}
