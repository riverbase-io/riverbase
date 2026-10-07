use crate::base::EngineContext;
use crate::base::RiverbaseResult;
use crate::base::ScopeMeta;
use crate::openapi_meta::OpenApiMeta;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Command Kind enumeration.
pub enum CommandKind {
    /// Collection.
    Collection,
    /// Object.
    Object,
    /// Object link.
    ObjectLink,
    /// Object hook.
    ObjectHook,
}

/// Authorization requirement declared next to command resources ([SEC-08]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CommandAuthz {
    /// Must be authorized by the active command policy / Casbin pack.
    #[default]
    Required,
    /// Explicitly public command (reason + sign-off, same bar as query `policy: public`).
    Public {
        /// Why this command is public.
        reason: String,
        /// Reviewer or decision identifier that signed off public access.
        signoff: String,
    },
}

#[derive(Debug, Clone)]
/// Command meta structure.
pub struct CommandMeta {
    /// Key.
    pub key: String,
    /// Title.
    pub title: String,
    /// Human-readable description (Python `Command.Meta.description`).
    pub description: Option<String>,
    /// Kind.
    pub kind: CommandKind,
    /// Resources.
    pub resources: Vec<String>,
    /// Scope.
    pub scope: ScopeMeta,
    /// Register `GET /{cmdkey}:link/{link_token}` (signed payload in token).
    pub allow_link_method: bool,
    /// Register `GET /{cmdkey}:hook/{auth_token_with_aggroot_scope_and_payload}` (payload from query params).
    pub allow_hook_method: bool,
    /// When true, the command may expose typed convenience methods on a domain command engine.
    pub engine_method: bool,
    /// Whitelist of deployment zones where this command route is registered.
    pub allowed_zones: Vec<String>,
    /// OpenAPI catalog / explorer overrides for HTTP route documentation.
    pub openapi: OpenApiMeta,
    /// Success envelope `data` map key (Python response type / DomainResponse key).
    pub response_type: String,
    /// Command authorization requirement ([SEC-08]).
    pub authz: CommandAuthz,
    /// Declarative profile-role requirements ([APP-01] / D12). Checked against
    /// [`EngineContext::roles`] (profile-store roles only — not IAM realm roles).
    pub roles_required: Vec<String>,
    /// Skip `_tenant` membership checks and rely on Casbin / `authz` only.
    pub tenant_scope_exempt: bool,
    /// Why this command is tenant-scope exempt.
    pub tenant_scope_reason: Option<String>,
    /// Reviewer or decision identifier that signed off tenant-scope exemption.
    pub tenant_scope_signoff: Option<String>,
}

impl CommandMeta {
    /// Whether this command allocates a new resource id (`:post` / Python `resource_init`).
    pub fn resource_init(&self) -> bool {
        matches!(self.kind, CommandKind::Collection)
    }

    /// Build the per-command info document served at `GET …/{cmdkey}.meta`.
    pub fn command_info_document(&self, schema: Value, response_schema: Value) -> Value {
        json!({
            "key": self.key,
            "name": self.title,
            "description": self.description.as_deref().unwrap_or(""),
            "schema": schema,
            "response_schema": response_schema,
            "resources": self.resources,
        })
    }

    /// Set description and return self.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Object.
    pub fn object(key: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            title: title.into(),
            description: None,
            kind: CommandKind::Object,
            resources: Vec::new(),
            scope: ScopeMeta::none(),
            allow_link_method: false,
            allow_hook_method: false,
            engine_method: true,
            allowed_zones: Vec::new(),
            openapi: OpenApiMeta::default(),
            response_type: crate::base::DEFAULT_RESPONSE_TYPE.to_string(),
            authz: CommandAuthz::Required,
            roles_required: Vec::new(),
            tenant_scope_exempt: false,
            tenant_scope_reason: None,
            tenant_scope_signoff: None,
        }
    }

    /// Collection.
    pub fn collection(key: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            title: title.into(),
            description: None,
            kind: CommandKind::Collection,
            resources: Vec::new(),
            scope: ScopeMeta::none(),
            allow_link_method: false,
            allow_hook_method: false,
            engine_method: true,
            allowed_zones: Vec::new(),
            openapi: OpenApiMeta::default(),
            response_type: crate::base::DEFAULT_RESPONSE_TYPE.to_string(),
            authz: CommandAuthz::Required,
            roles_required: Vec::new(),
            tenant_scope_exempt: false,
            tenant_scope_reason: None,
            tenant_scope_signoff: None,
        }
    }

    /// Back-compat alias for [`Self::object`].
    pub fn object_exec(key: impl Into<String>, title: impl Into<String>) -> Self {
        Self::object(key, title)
    }

    /// Back-compat alias for [`Self::object`].
    pub fn exec(key: impl Into<String>, title: impl Into<String>) -> Self {
        Self::object(key, title)
    }

    /// Back-compat alias for [`Self::collection`].
    pub fn init(key: impl Into<String>, title: impl Into<String>) -> Self {
        Self::collection(key, title)
    }

    /// Set resources and return self.
    pub fn with_resources(
        mut self,
        resources: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.resources = resources.into_iter().map(Into::into).collect();
        self
    }

    /// Set response type and return self.
    pub fn with_response_type(mut self, response_type: impl Into<String>) -> Self {
        self.response_type = response_type.into();
        self
    }

    /// Set scope and return self.
    pub fn with_scope(mut self, scope: ScopeMeta) -> Self {
        self.scope = scope;
        self
    }

    /// Set scope schema and return self.
    pub fn with_scope_schema(mut self, schema: Value) -> Self {
        self.scope = ScopeMeta::required(Some(schema));
        self
    }

    /// Set link method and return self.
    pub fn with_link_method(mut self) -> Self {
        self.allow_link_method = true;
        self
    }

    /// Set hook method and return self.
    pub fn with_hook_method(mut self) -> Self {
        self.allow_hook_method = true;
        self
    }

    /// Set engine method and return self.
    pub fn with_engine_method(mut self, engine_method: bool) -> Self {
        self.engine_method = engine_method;
        self
    }

    /// Set openapi tag and return self.
    pub fn with_openapi_tag(mut self, tag: impl Into<String>) -> Self {
        self.openapi.tag = Some(tag.into());
        self
    }

    /// Set openapi explorer and return self.
    pub fn with_openapi_explorer(mut self, explorer: bool) -> Self {
        self.openapi.explorer = Some(explorer);
        self
    }

    /// Set openapi internal and return self.
    pub fn with_openapi_internal(mut self, internal: bool) -> Self {
        self.openapi.internal = Some(internal);
        self
    }

    /// Set openapi deprecated and return self.
    pub fn with_openapi_deprecated(mut self, deprecated: bool) -> Self {
        self.openapi.deprecated = deprecated;
        self
    }

    /// Set authz and return self.
    pub fn with_authz(mut self, authz: CommandAuthz) -> Self {
        self.authz = authz;
        self
    }

    /// Set roles required and return self.
    pub fn with_roles_required(
        mut self,
        roles: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.roles_required = roles.into_iter().map(Into::into).collect();
        self
    }

    /// Disable the `_tenant` filter for this command (Casbin / `authz` still apply).
    pub fn with_tenant_scope_exempt(
        mut self,
        reason: impl Into<String>,
        signoff: impl Into<String>,
    ) -> Self {
        self.tenant_scope_exempt = true;
        self.tenant_scope_reason = Some(reason.into());
        self.tenant_scope_signoff = Some(signoff.into());
        self
    }

    /// Whether this command skips `_tenant` membership checks.
    pub fn tenant_scope_exempt(&self) -> bool {
        self.tenant_scope_exempt
    }

    /// Set allowed zones and return self.
    pub fn with_allowed_zones(
        mut self,
        zones: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.allowed_zones = zones.into_iter().map(Into::into).collect();
        self
    }

    /// Whether this command route should be registered for the configured deployment zones.
    pub fn zone_allowed(&self, api_zone: &[String]) -> bool {
        crate::util::zone_allowed(&self.allowed_zones, api_zone)
    }
}

/// Enforce declarative `roles_required` against profile roles on the engine context ([D12]).
///
/// IAM realm roles must not be consulted here — they are reserved for `api_zone` authorization.
pub fn authorize_command_roles(ctx: &EngineContext, meta: &CommandMeta) -> RiverbaseResult<()> {
    if meta.roles_required.is_empty() {
        return Ok(());
    }
    let granted: std::collections::HashSet<&str> = ctx.roles.iter().map(String::as_str).collect();
    let missing: Vec<&str> = meta
        .roles_required
        .iter()
        .map(String::as_str)
        .filter(|role| !granted.contains(role))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(crate::errors::AUT_003.with_data(serde_json::json!({
        "roles_required": meta.roles_required,
        "subject": meta.key,
        "missing": missing,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_info_document_matches_python_shape() {
        let meta = CommandMeta::collection("create-todo", "Create Todo")
            .with_description("Creates a todo")
            .with_resources(["todo"]);
        let doc = meta.command_info_document(
            json!({"type": "object"}),
            json!({"type": "object", "additionalProperties": true}),
        );
        assert_eq!(doc["key"], "create-todo");
        assert_eq!(doc["name"], "Create Todo");
        assert_eq!(doc["description"], "Creates a todo");
        assert_eq!(doc["resources"], json!(["todo"]));
        assert!(doc.get("schema").is_some());
        assert!(doc.get("response_schema").is_some());
    }

    #[test]
    fn tenant_scope_exempt_is_opt_in() {
        let meta = CommandMeta::object("list-catalogs", "List catalogs")
            .with_tenant_scope_exempt("IDM has no _tenant", "SEC-REVIEW-1");
        assert!(meta.tenant_scope_exempt());
        assert_eq!(
            meta.tenant_scope_reason.as_deref(),
            Some("IDM has no _tenant")
        );
    }
}
