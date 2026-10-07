use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use uuid::Uuid;

/// JWT / session claim keys tried when resolving audit `profile_id` (before `sub`).
const PROFILE_ID_CLAIM_KEYS: &[&str] = &[
    "profile_id",
    "profileId",
    "current_profile_id",
    "active_profile_id",
];

/// Parse a UUID from a JSON claim value (typically a string).
pub fn uuid_from_json(value: &Value) -> Option<Uuid> {
    value.as_str().and_then(|s| Uuid::parse_str(s).ok())
}

/// Resolve profile UUID from OIDC claims (`profile_id`, …) or fall back to `sub` when it is a UUID.
pub fn profile_id_from_claims_and_sub(claims: &Value, sub: &str) -> Option<Uuid> {
    for key in PROFILE_ID_CLAIM_KEYS {
        if let Some(id) = claims.get(*key).and_then(uuid_from_json) {
            return Some(id);
        }
    }
    Uuid::parse_str(sub).ok()
}

use super::engine::{EngineContext, InvocationMode};
use super::ids::CommandId;
use super::scope::ScopeMap;
use super::tenant::{TenantAccess, TenantPolicyResolver};
use crate::datastore::CommandUnitOfWork;

/// Authenticated actor identity for domain audit columns (`_creator`, `_updater`, `_tenant`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditActor {
    /// Active profile identifier.
    pub profile_id: Option<Uuid>,
    /// User id.
    pub user_id: Option<Uuid>,
    /// Tenant stamped into `_tenant` on persisted rows. Not the Riverbase domain namespace or auth realm.
    pub tenant: Option<Uuid>,
}

impl AuditActor {
    /// Construct a new value.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set profile id and return self.
    pub fn with_profile_id(mut self, id: Uuid) -> Self {
        self.profile_id = Some(id);
        self
    }

    /// Set user id and return self.
    pub fn with_user_id(mut self, id: Uuid) -> Self {
        self.user_id = Some(id);
        self
    }

    /// Set tenant id and return self.
    pub fn with_tenant_id(mut self, tenant: Uuid) -> Self {
        self.tenant = Some(tenant);
        self
    }

    /// Stamp `_creator` / `_updater` from JWT `sub` and optional profile claims.
    pub fn from_subject_and_claims(sub: &str, claims: &Value, tenant: Option<Uuid>) -> Self {
        let mut actor = Self::new();
        actor.profile_id = profile_id_from_claims_and_sub(claims, sub);
        actor.user_id = user_id_from_claims(claims);
        actor.tenant = tenant;
        actor
    }
}

/// Resolve platform user UUID from OIDC claims (`sub`).
pub fn user_id_from_claims(claims: &Value) -> Option<Uuid> {
    claims.get("sub").and_then(uuid_from_json)
}

/// Per-command context for aggregate writes (derived from [`EngineContext`] + command envelope).
#[derive(Debug, Clone)]
pub struct AggregateContext {
    /// Namespace.
    pub namespace: String,
    /// Title.
    pub title: Option<String>,
    /// Correlation id.
    pub correlation_id: Option<String>,
    /// Trace id.
    pub trace_id: Option<String>,
    /// Actor.
    pub actor: AuditActor,
    /// Roles.
    pub roles: Vec<String>,
    /// Claims.
    pub claims: BTreeMap<String, Value>,
    /// Idempotency key.
    pub idempotency_key: Option<String>,
    /// Deny unknown fields.
    pub deny_unknown_fields: bool,
    /// Causation id.
    pub causation_id: Option<String>,
    /// Parent command id.
    pub parent_command_id: Option<String>,
    /// Invocation depth.
    pub invocation_depth: u16,
    /// Invocation mode.
    pub invocation_mode: InvocationMode,
    /// Service capability.
    pub service_capability: Option<String>,
    /// Cmd id.
    pub cmd_id: CommandId,
    /// Domain context row id (`command_log.context`, `activity_log.context`).
    pub context_id: Uuid,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
    /// Scope.
    pub scope: ScopeMap,
    /// Command-scoped persistence handle (set by the command engine after begin).
    pub unit_of_work: CommandUnitOfWork,
    /// `_tenant IN …` access list copied from [`EngineContext`].
    pub tenant_access: TenantAccess,
    /// Organization id copied from [`EngineContext`].
    pub organization_id: Option<Uuid>,
    /// When true, skip `_tenant` membership checks (Casbin-only). Stamp still writes.
    pub tenant_scope_exempt: bool,
    /// Portal stamp/access policies copied from [`EngineContext`].
    pub tenant_policies: Option<TenantPolicyResolver>,
}

impl AggregateContext {
    /// Build from engine.
    pub fn from_engine(engine: &EngineContext, cmd_id: CommandId, scope: ScopeMap) -> Self {
        Self {
            namespace: engine.namespace.clone(),
            title: engine.title.clone(),
            correlation_id: engine.correlation_id.clone(),
            trace_id: engine.trace_id.clone(),
            actor: engine.actor.clone(),
            roles: engine.roles.clone(),
            claims: engine.claims.clone(),
            idempotency_key: engine.idempotency_key.clone(),
            deny_unknown_fields: engine.deny_unknown_fields,
            causation_id: engine.causation_id.clone(),
            parent_command_id: engine.parent_command_id.clone(),
            invocation_depth: engine.invocation_depth,
            invocation_mode: engine.invocation_mode.clone(),
            service_capability: engine.service_capability.clone(),
            cmd_id,
            context_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            scope,
            unit_of_work: CommandUnitOfWork::non_transactional(),
            tenant_access: engine.tenant_access.clone(),
            organization_id: engine.organization_id,
            tenant_scope_exempt: false,
            tenant_policies: engine.tenant_policies.clone(),
        }
    }

    /// Stamp policy name, or the default when the portal did not install one.
    pub fn stamp_policy(&self) -> &str {
        self.tenant_policies
            .as_ref()
            .map(|policies| policies.stamp_policy())
            .unwrap_or(super::tenant::DEFAULT_TENANT_STAMP_POLICY)
    }

    /// Tenant identifier copied from auth context (`_tenant`).
    pub fn tenant(&self) -> Option<Uuid> {
        self.actor.tenant
    }

    /// Require a UUID tenant identifier on this command context.
    pub fn require_tenant(&self) -> crate::base::RiverbaseResult<Uuid> {
        self.tenant()
            .ok_or_else(|| crate::errors::DOM_045.with_data("missing _tenant"))
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

    /// Whether `_tenant` membership checks should run for this command.
    pub fn tenant_filter_active(&self) -> bool {
        !self.tenant_scope_exempt && !self.effective_tenant_access().is_all()
    }

    /// Whether a persisted row is visible under the current access list / exempt flag.
    pub fn row_visible(&self, row: &serde_json::Value) -> bool {
        if !self.tenant_filter_active() {
            return true;
        }
        self.effective_tenant_access().allows_row(row)
    }

    /// Reconstruct HTTP/command [`EngineContext`] for cross-domain invocations (sagas).
    pub fn to_engine_context(&self) -> EngineContext {
        EngineContext {
            namespace: self.namespace.clone(),
            title: self.title.clone(),
            correlation_id: self.correlation_id.clone(),
            trace_id: self.trace_id.clone(),
            actor: self.actor.clone(),
            roles: self.roles.clone(),
            claims: self.claims.clone(),
            idempotency_key: self.idempotency_key.clone(),
            deny_unknown_fields: self.deny_unknown_fields,
            causation_id: Some(self.cmd_id.0.clone()),
            parent_command_id: Some(self.cmd_id.0.clone()),
            invocation_depth: self.invocation_depth,
            invocation_mode: self.invocation_mode.clone(),
            service_capability: self.service_capability.clone(),
            tenant_access: self.tenant_access.clone(),
            organization_id: self.organization_id,
            profile_id: self.actor.profile_id,
            user_id: self.actor.user_id,
            tenant_policies: self.tenant_policies.clone(),
        }
    }
}

/// Shared Riverbase domain row metadata (`_id`, `_created`, …).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainFields {
    /// Row identifier.
    pub id: Uuid,
    /// Creation timestamp.
    pub created: DateTime<Utc>,
    /// Updated.
    pub updated: Option<DateTime<Utc>>,
    /// Creating principal identifier, if known.
    pub creator: Option<Uuid>,
    /// Updater.
    pub updater: Option<Uuid>,
    /// Tenant copied to `_tenant` when the row is created. Not the domain namespace or auth realm.
    pub tenant: Option<Uuid>,
    /// Etag.
    pub etag: Option<Uuid>,
    /// Deleted.
    pub deleted: Option<DateTime<Utc>>,
}

impl Default for DomainFields {
    fn default() -> Self {
        Self::new()
    }
}

impl DomainFields {
    /// Construct a new value.
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4(),
            created: Utc::now(),
            updated: None,
            creator: None,
            updater: None,
            tenant: None,
            etag: None,
            deleted: None,
        }
    }

    /// Set id and return self.
    pub fn with_id(id: Uuid) -> Self {
        let mut fields = Self::new();
        fields.id = id;
        fields
    }

    /// Build from context.
    pub fn from_context(ctx: &AggregateContext, id: Uuid) -> Self {
        Self {
            id,
            created: ctx.timestamp,
            updated: None,
            creator: ctx.actor.profile_id,
            updater: None,
            tenant: ctx.tenant(),
            etag: Some(Self::generate_etag()),
            deleted: None,
        }
    }

    /// Create stamp for inserts: `_updated` equals `_created` so incremental sync sees new rows.
    pub fn for_insert(ctx: &AggregateContext, id: Uuid) -> Self {
        let mut fields = Self::from_context(ctx, id);
        fields.updated = Some(fields.created);
        fields
    }

    /// Touch from context.
    pub fn touch_from_context(&mut self, ctx: &AggregateContext) {
        self.updated = Some(ctx.timestamp);
        self.updater = ctx.actor.profile_id;
        self.etag = Some(Self::generate_etag());
    }

    /// Touch updated.
    pub fn touch_updated(&mut self) {
        self.updated = Some(Utc::now());
    }

    /// Generate etag.
    pub fn generate_etag() -> Uuid {
        Uuid::new_v4()
    }

    /// Merge created into.
    pub fn merge_created_into(&self, data: &mut Value) {
        let obj = data
            .as_object_mut()
            .expect("audit merge requires JSON object");
        insert_audit_fields(obj, self);
    }

    /// Merge updated into.
    pub fn merge_updated_into(&self, data: &mut Value) {
        let obj = data
            .as_object_mut()
            .expect("audit merge requires JSON object");
        if let Some(updated) = self.updated {
            obj.insert("_updated".to_string(), json!(updated));
        }
        if let Some(updater) = self.updater {
            obj.insert("_updater".to_string(), json!(updater.to_string()));
        }
        if let Some(etag) = self.etag {
            obj.insert("_etag".to_string(), json!(etag.to_string()));
        }
    }
}

/// Ensure JSON state exposes `_etag` for [`crate::command::aggregate::Aggregate::update`] CAS.
///
/// Domain `row_to_json` helpers often omit audit columns; `pg_domain_entity!` uses this so
/// optimistic concurrency still works against the persisted `_etag` column.
pub fn with_json_etag(mut data: Value, etag: Uuid) -> Value {
    if let Some(obj) = data.as_object_mut() {
        obj.entry("_etag".to_string())
            .or_insert_with(|| json!(etag.to_string()));
    }
    data
}

/// Ensure JSON state exposes `_tenant` for aggregate tenant matching.
///
/// Domain `row_to_json` helpers often omit audit columns. Without this, object
/// commands treat an existing same-tenant row as missing (`CMD-007`).
pub fn with_json_tenant(mut data: Value, tenant: Option<Uuid>) -> Value {
    if let (Some(tenant), Some(obj)) = (tenant, data.as_object_mut()) {
        obj.entry("_tenant".to_string())
            .or_insert_with(|| json!(tenant.to_string()));
    }
    data
}

/// Restore `_etag` and `_tenant` after a domain `row_to_json` that omitted them.
pub fn with_json_domain_meta(data: Value, etag: Uuid, tenant: Option<Uuid>) -> Value {
    with_json_tenant(with_json_etag(data, etag), tenant)
}

fn insert_audit_fields(obj: &mut Map<String, Value>, fields: &DomainFields) {
    obj.insert("_created".to_string(), json!(fields.created));
    if let Some(updated) = fields.updated {
        obj.insert("_updated".to_string(), json!(updated));
    }
    if let Some(creator) = fields.creator {
        obj.insert("_creator".to_string(), json!(creator.to_string()));
    }
    if let Some(updater) = fields.updater {
        obj.insert("_updater".to_string(), json!(updater.to_string()));
    }
    if let Some(tenant) = fields.tenant {
        obj.insert("_tenant".to_string(), json!(tenant.to_string()));
    }
    if let Some(etag) = fields.etag {
        obj.insert("_etag".to_string(), json!(etag.to_string()));
    }
    if let Some(deleted) = fields.deleted {
        obj.insert("_deleted".to_string(), json!(deleted));
    }
}

/// Build domain fields from a JSON payload (e.g. after [`DomainFields::merge_created_into`]).
pub fn domain_fields_from_payload(data: &Value, id: Uuid) -> DomainFields {
    let timestamp = parse_timestamp(data.get("_created")).unwrap_or_else(Utc::now);
    let mut actor = AuditActor::new();
    if let Some(creator) = parse_uuid(data.get("_creator")) {
        actor.profile_id = Some(creator);
    }
    if let Some(tenant) = parse_uuid(data.get("_tenant")) {
        actor.tenant = Some(tenant);
    }
    let engine = EngineContext::new("").with_actor(actor);
    let ctx = AggregateContext::from_engine(&engine, CommandId::new(), ScopeMap::new());
    let mut ctx = ctx;
    ctx.timestamp = timestamp;
    domain_fields_from_json(data, id, &ctx)
}

/// Build domain fields for entity insert, ensuring `_updated` is set (defaults to `_created`).
pub fn domain_fields_for_entity_insert(data: &Value, id: Uuid) -> DomainFields {
    let mut fields = domain_fields_from_payload(data, id);
    fields.updated = fields.updated.or(Some(fields.created));
    fields
}

/// Build domain fields for insert, preferring audit keys present in `data` when set.
pub fn domain_fields_from_json(data: &Value, id: Uuid, ctx: &AggregateContext) -> DomainFields {
    let mut fields = DomainFields::from_context(ctx, id);
    if let Some(created) = parse_timestamp(data.get("_created")) {
        fields.created = created;
    }
    if let Some(updated) = parse_timestamp(data.get("_updated")) {
        fields.updated = Some(updated);
    }
    if let Some(creator) = parse_uuid(data.get("_creator")) {
        fields.creator = Some(creator);
    }
    if let Some(updater) = parse_uuid(data.get("_updater")) {
        fields.updater = Some(updater);
    }
    if let Some(tenant) = parse_uuid(data.get("_tenant")) {
        fields.tenant = Some(tenant);
    }
    if let Some(etag) = parse_uuid(data.get("_etag")) {
        fields.etag = Some(etag);
    }
    if let Some(deleted) = parse_timestamp(data.get("_deleted")) {
        fields.deleted = Some(deleted);
    }
    fields
}

fn parse_uuid(value: Option<&Value>) -> Option<Uuid> {
    value
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
}

fn parse_timestamp(value: Option<&Value>) -> Option<DateTime<Utc>> {
    value.and_then(|v| {
        v.as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_engine_keeps_stamp_policy() {
        let org = Uuid::new_v4();
        let mut engine = EngineContext::new("rfx.setting")
            .with_organization_id(org)
            .with_tenant_id(org);
        engine.tenant_policies = Some(crate::base::TenantPolicyResolver::new(
            crate::base::ACCESS_SYSTEM,
            crate::base::STAMP_PROFILE,
        ));
        let ctx = AggregateContext::from_engine(&engine, CommandId::new(), ScopeMap::new());
        assert_eq!(ctx.stamp_policy(), crate::base::STAMP_PROFILE);
        let nested = ctx.to_engine_context();
        assert_eq!(
            nested
                .tenant_policies
                .as_ref()
                .map(|policies| policies.stamp_policy()),
            Some(crate::base::STAMP_PROFILE)
        );
    }

    #[test]
    fn for_insert_sets_updated_equal_created() {
        let engine = EngineContext::new("riverbase.todo")
            .with_actor(AuditActor::new().with_profile_id(Uuid::new_v4()));
        let ctx = AggregateContext::from_engine(&engine, CommandId::new(), ScopeMap::new());
        let id = Uuid::new_v4();
        let fields = DomainFields::for_insert(&ctx, id);
        assert_eq!(fields.updated, Some(fields.created));
    }

    #[test]
    fn domain_fields_for_entity_insert_defaults_updated_to_created() {
        let id = Uuid::new_v4();
        let fields = domain_fields_for_entity_insert(&json!({}), id);
        assert_eq!(fields.updated, Some(fields.created));
    }

    #[test]
    fn domain_fields_for_entity_insert_preserves_stamped_audit_keys() {
        let creator = Uuid::new_v4();
        let id = Uuid::new_v4();
        let created = "2026-08-01T10:00:00Z";
        let updated = "2026-08-01T11:00:00Z";
        let data = json!({
            "_created": created,
            "_updated": updated,
            "_creator": creator.to_string(),
        });
        let fields = domain_fields_for_entity_insert(&data, id);
        assert_eq!(fields.creator, Some(creator));
        assert_eq!(
            fields
                .updated
                .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
            Some(updated.to_string())
        );
    }

    #[test]
    fn from_context_sets_creator_and_tenant() {
        let tenant = Uuid::new_v4();
        let engine = EngineContext::new("riverbase.todo").with_actor(
            AuditActor::new()
                .with_tenant_id(tenant)
                .with_profile_id(Uuid::new_v4()),
        );
        let ctx = AggregateContext::from_engine(&engine, CommandId::new(), ScopeMap::new());
        let id = Uuid::new_v4();
        let fields = DomainFields::from_context(&ctx, id);
        assert_eq!(fields.id, id);
        assert_eq!(fields.creator, engine.actor.profile_id);
        assert_eq!(fields.tenant, Some(tenant));
        assert!(fields.etag.is_some());
    }

    #[test]
    fn for_insert_overwrites_payload_tenant() {
        let tenant = Uuid::new_v4();
        let engine = EngineContext::new("riverbase.todo").with_tenant_id(tenant);
        let ctx = AggregateContext::from_engine(&engine, CommandId::new(), ScopeMap::new());
        let fields = DomainFields::for_insert(&ctx, Uuid::new_v4());
        let mut data = json!({ "_tenant": Uuid::new_v4().to_string(), "title": "x" });
        fields.merge_created_into(&mut data);
        assert_eq!(data.get("_tenant"), Some(&json!(tenant.to_string())));
    }

    #[test]
    fn tenant_is_not_domain_namespace() {
        let engine = EngineContext::new("exp.catalog");
        let ctx = AggregateContext::from_engine(&engine, CommandId::new(), ScopeMap::new());
        assert_eq!(ctx.tenant(), None);
        assert!(ctx.require_tenant().is_err());
    }

    #[test]
    fn row_visible_respects_access_all_and_exempt() {
        let tenant = Uuid::new_v4();
        let other = Uuid::new_v4();
        let engine = EngineContext::new("exp.catalog").with_tenant_id(tenant);
        let mut ctx = AggregateContext::from_engine(&engine, CommandId::new(), ScopeMap::new());
        let own = json!({ "_tenant": tenant.to_string() });
        let foreign = json!({ "_tenant": other.to_string() });
        assert!(ctx.row_visible(&own));
        assert!(!ctx.row_visible(&foreign));

        ctx.tenant_access = TenantAccess::All;
        assert!(ctx.row_visible(&foreign));

        ctx.tenant_access = TenantAccess::tenants(vec![tenant]);
        ctx.tenant_scope_exempt = true;
        assert!(ctx.row_visible(&foreign));
    }

    #[test]
    fn merge_created_into_json() {
        let engine = EngineContext::new("riverbase.todo");
        let ctx = AggregateContext::from_engine(&engine, CommandId::new(), ScopeMap::new());
        let id = Uuid::new_v4();
        let fields = DomainFields::from_context(&ctx, id);
        let mut data = json!({ "title": "x" });
        fields.merge_created_into(&mut data);
        assert!(data.get("_created").is_some());
        assert!(data.get("_etag").is_some());
    }

    #[test]
    fn with_json_etag_fills_missing_token() {
        let etag = Uuid::new_v4();
        let data = with_json_etag(json!({ "id": "1" }), etag);
        assert_eq!(data.get("_etag"), Some(&json!(etag.to_string())));
        let existing = Uuid::new_v4();
        let data = with_json_etag(json!({ "_etag": existing.to_string() }), etag);
        assert_eq!(data.get("_etag"), Some(&json!(existing.to_string())));
    }

    #[test]
    fn with_json_tenant_fills_missing_and_keeps_explicit() {
        let tenant = Uuid::new_v4();
        let data = with_json_tenant(json!({ "id": "1" }), Some(tenant));
        assert_eq!(data.get("_tenant"), Some(&json!(tenant.to_string())));
        assert_eq!(
            with_json_tenant(json!({ "id": "1" }), None).get("_tenant"),
            None
        );
        let existing = Uuid::new_v4();
        let data = with_json_tenant(json!({ "_tenant": existing.to_string() }), Some(tenant));
        assert_eq!(data.get("_tenant"), Some(&json!(existing.to_string())));
    }

    #[test]
    fn with_json_domain_meta_fills_etag_and_tenant() {
        let etag = Uuid::new_v4();
        let tenant = Uuid::new_v4();
        let data = with_json_domain_meta(json!({ "title": "x" }), etag, Some(tenant));
        assert_eq!(data.get("_etag"), Some(&json!(etag.to_string())));
        assert_eq!(data.get("_tenant"), Some(&json!(tenant.to_string())));
    }
}
