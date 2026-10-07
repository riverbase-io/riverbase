use std::collections::HashMap;
use std::sync::Arc;

use crate::base::{
    item_envelope, list_envelope, report_envelope, Engine, EngineContext, EngineKind,
    RiverbaseResult, ScopeMeta, TenantAccess,
};
use crate::logstore::{DomainLogStore, QueryLogStatus};
use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::Semaphore;
use tracing::info;
use uuid::Uuid;

use super::log::append_query_log;
use super::lower::{
    lower_item, lower_list, pagination_meta, project_and_relabel, with_identifier_filter,
};
use super::primitives::QueryRequest;
use super::resource::{PolicyDecision, QueryAccess, QueryResource, QueryRouteMeta, ReportOutput};
use super::session::QuerySession;

/// Server-side policy-filter source combined with each resource's `scope_policy` to populate
/// [`DataQuery::policy_filter`](crate::datastore::dsl::DataQuery::policy_filter). The default wiring
/// installs no provider; a Casbin-backed implementation can be supplied later via
/// [`QueryEngineArgs::with_policy_provider`].
pub trait PolicyFilterProvider: Send + Sync {
    /// Policy filter.
    fn policy_filter(
        &self,
        ctx: &EngineContext,
        resource: &str,
        access: QueryAccess,
        url_scope: Option<&Value>,
    ) -> RiverbaseResult<PolicyDecision>;
}

/// Resources wired into a query engine (no namespace).
///
/// Register resources with [`register`](Self::register) before [`QueryEngine::spawn`].
#[derive(Clone)]
pub struct QueryEngineArgs {
    /// Resources.
    pub resources: HashMap<String, Arc<dyn QueryResource>>,
    /// Logstore.
    pub logstore: DomainLogStore,
    /// Optional server-side policy-filter source (e.g. Casbin); `None` disables the contribution.
    pub policy_provider: Option<Arc<dyn PolicyFilterProvider>>,
    /// Domain namespace for spawn-time logging (from [`EngineContext::namespace`]).
    pub engine_name: Option<String>,
    /// Maximum page size clients may request ([DAT-09]).
    pub query_max_limit: u64,
    /// Cap exact counts above this threshold ([DAT-09]).
    pub query_count_max_rows: u64,
    duplicate_resources: Vec<String>,
}

impl QueryEngineArgs {
    /// Construct a new value.
    pub fn new(logstore: DomainLogStore) -> Self {
        Self {
            resources: HashMap::new(),
            logstore,
            policy_provider: None,
            engine_name: None,
            query_max_limit: crate::config::default_query_max_limit(),
            query_count_max_rows: crate::config::default_query_count_max_rows(),
            duplicate_resources: Vec::new(),
        }
    }

    /// Stamp the owning domain namespace onto spawn logs.
    pub fn apply_engine_context(&mut self, ctx: &EngineContext) {
        self.engine_name = Some(ctx.namespace().to_string());
    }

    /// Register.
    pub fn register(&mut self, resource: Arc<dyn QueryResource>) {
        let name = resource.name().to_string();
        if self.resources.insert(name.clone(), resource).is_some() {
            self.duplicate_resources.push(name);
        }
    }

    /// Install a server-side [`PolicyFilterProvider`] (combined with per-resource `scope_policy`).
    pub fn with_policy_provider(mut self, provider: Arc<dyn PolicyFilterProvider>) -> Self {
        self.policy_provider = Some(provider);
        self
    }
}

async fn run_execute(
    args: &QueryEngineArgs,
    ctx: &EngineContext,
    query_resource: &str,
    access: QueryAccess,
    request: QueryRequest,
    item_id: Option<&str>,
) -> RiverbaseResult<Value> {
    let _ = ctx.namespace();
    let resource = args
        .resources
        .get(query_resource)
        .ok_or_else(|| crate::errors::QRY_001.with_data(query_resource.to_string()))?;

    let iface = resource.interface();
    if let Some(text) = request
        .text
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if !iface.allow_text_search() {
            return Err(crate::errors::QRY_120.with_data(text.to_string()));
        }
    }

    if matches!(access, QueryAccess::List | QueryAccess::Report)
        && (request.limit == 0 || request.limit > args.query_max_limit)
    {
        return Err(crate::errors::QRY_130.with_data(format!(
            "limit {} (max {})",
            request.limit, args.query_max_limit
        )));
    }

    // Server-side authorization restriction:
    // (e.g. casbin). Applied ahead of the client filter; not validated against the interface.
    let required_roles = iface.roles_required();
    if !required_roles.is_empty() {
        let granted: std::collections::HashSet<&str> =
            ctx.roles.iter().map(String::as_str).collect();
        let missing: Vec<&str> = required_roles
            .iter()
            .copied()
            .filter(|role| !granted.contains(role))
            .collect();
        if !missing.is_empty() {
            return Err(crate::errors::AUT_003.with_data(serde_json::json!({
                "roles_required": required_roles,
                "subject": query_resource,
                "missing": missing,
            })));
        }
    }
    let policy_filter = if access == QueryAccess::Meta {
        None
    } else {
        let url_scope = request.scope.as_ref();
        let scope_policy = resource.policy_decision(ctx, url_scope)?;
        let provider_policy = match args.policy_provider.as_ref() {
            Some(provider) => provider.policy_filter(ctx, query_resource, access, url_scope)?,
            None => PolicyDecision::Unrestricted,
        };
        let mut decision = scope_policy.combine(provider_policy);
        decision = decision.combine(tenant_access_decision(ctx, resource.tenant_scope_exempt())?);
        if resource.policy_requirement().is_required()
            && matches!(decision, PolicyDecision::Unrestricted)
        {
            decision = PolicyDecision::Denied(serde_json::json!({
                "code": "QRY-124",
                "reason": "required policy did not produce a constraint",
            }));
        }
        decision.into_filter(query_resource)?
    };

    let context_id = Uuid::new_v4();
    let session = QuerySession::new(
        args.logstore.clone(),
        ctx.clone(),
        context_id,
        query_resource,
    );

    let outcome = match access {
        QueryAccess::List => {
            let mut query = lower_list(&request, iface, resource.binding())?;
            query.policy_filter = policy_filter;
            let (rows, total) = resource.execute_list_with_total(&query).await?;
            let rows = project_and_relabel(rows, &query);
            let result_count = rows.len().min(i32::MAX as usize) as i32;
            Ok((
                list_envelope(
                    serde_json::json!(rows),
                    pagination_meta(&request, total, Some(args.query_count_max_rows)),
                ),
                Some(result_count),
            ))
        }
        QueryAccess::Item => {
            let id = item_id.ok_or_else(|| crate::errors::QRY_002.with_data("item id required"))?;
            let query = lower_item(&request, iface, resource.binding())?;
            let mut query = with_identifier_filter(query, iface, resource.binding(), id)?;
            query.policy_filter = policy_filter;
            match resource.execute_item(&query, id).await? {
                Some(row) => {
                    let projected = project_and_relabel(vec![row], &query)
                        .into_iter()
                        .next()
                        .unwrap_or(Value::Null);
                    Ok((
                        item_envelope(
                            projected,
                            serde_json::json!({ "resource_id": id.to_string() }),
                        ),
                        Some(1),
                    ))
                }
                None => {
                    let code = resource.item_not_found_error_code();
                    if code == crate::errors::QRY_101.errcode {
                        Err(crate::errors::QRY_101.with_data(id.to_string()))
                    } else {
                        Err(crate::base::NotFoundError::with(
                            code,
                            "Query item was not found.",
                            id.to_string(),
                        ))
                    }
                }
            }
        }
        QueryAccess::Meta => Ok((item_envelope(resource.meta(), serde_json::json!({})), None)),
        QueryAccess::Report => {
            let mut query = lower_list(&request, iface, resource.binding())?;
            query.policy_filter = policy_filter;
            match resource
                .execute_report(ctx, &session, &request, &query)
                .await?
            {
                ReportOutput::Rows { rows, total } => {
                    let rows = project_and_relabel(rows, &query);
                    let result_count = rows.len().min(i32::MAX as usize) as i32;
                    Ok((
                        crate::base::success_envelope_typed(
                            crate::base::ENVELOPE_REPORT,
                            serde_json::json!(rows),
                            serde_json::json!({
                                "pagination": pagination_meta(
                                    &request,
                                    total,
                                    Some(args.query_count_max_rows),
                                )
                            }),
                        ),
                        Some(result_count),
                    ))
                }
                ReportOutput::Document(document) => {
                    Ok((report_envelope(document, serde_json::json!({})), None))
                }
            }
        }
    };

    match outcome {
        Ok((value, result_count)) => {
            spawn_query_log(
                args.logstore.clone(),
                ctx.clone(),
                query_resource.to_string(),
                access,
                request,
                item_id.map(str::to_string),
                QueryLogStatus::Success,
                result_count,
                None,
                Some(context_id),
            );
            Ok(value)
        }
        Err(err) => {
            let error_code = Some(err.errcode.as_str().to_string());
            spawn_query_log(
                args.logstore.clone(),
                ctx.clone(),
                query_resource.to_string(),
                access,
                request,
                item_id.map(str::to_string),
                QueryLogStatus::Errored,
                None,
                error_code,
                Some(context_id),
            );
            Err(err)
        }
    }
}

fn tenant_access_decision(ctx: &EngineContext, exempt: bool) -> RiverbaseResult<PolicyDecision> {
    if exempt {
        return Ok(PolicyDecision::Unrestricted);
    }
    match ctx.effective_tenant_access() {
        TenantAccess::All => Ok(PolicyDecision::Unrestricted),
        access if access.is_empty() => Err(crate::errors::QRY_141.with_data("empty tenant access")),
        access => Ok(PolicyDecision::Constrained(
            access
                .filter_expr()
                .expect("non-empty tenant access has an IN-list filter"),
        )),
    }
}

fn spawn_query_log(
    logstore: DomainLogStore,
    ctx: EngineContext,
    query_resource: String,
    access: QueryAccess,
    request: QueryRequest,
    item_id: Option<String>,
    status: QueryLogStatus,
    result_count: Option<i32>,
    error_code: Option<String>,
    context_id: Option<Uuid>,
) {
    tokio::spawn(async move {
        let _ = append_query_log(
            &logstore,
            &ctx,
            &query_resource,
            access,
            &request,
            item_id.as_deref(),
            status,
            result_count,
            error_code,
            context_id,
        )
        .await;
    });
}

/// Query engine with bounded direct concurrency (no per-request actor mailbox queue).
#[derive(Clone)]
pub struct QueryEngine {
    args: Arc<QueryEngineArgs>,
    concurrency: Arc<Semaphore>,
}

impl QueryEngine {
    /// Spawn.
    pub async fn spawn(args: QueryEngineArgs) -> RiverbaseResult<Self> {
        Self::spawn_with_size(1, args).await
    }

    /// Spawn with size.
    pub async fn spawn_with_size(size: usize, args: QueryEngineArgs) -> RiverbaseResult<Self> {
        if !args.duplicate_resources.is_empty() {
            return Err(crate::errors::QRY_122.with_data(args.duplicate_resources.join(",")));
        }
        for resource in args.resources.values() {
            let requirement = resource.policy_requirement();
            if requirement.is_required() && !resource.policy_filter_enforced() {
                return Err(crate::errors::QRY_123.with_data(resource.name().to_string()));
            }
            if requirement.is_required()
                && !resource.has_scope_policy()
                && resource.tenant_scope_exempt()
            {
                return Err(crate::errors::QRY_142.with_data(resource.name().to_string()));
            }
            if let (Some(reason), Some(signoff)) =
                (requirement.public_reason(), requirement.public_signoff())
            {
                tracing::warn!(
                    engine = args.engine_name.as_deref().unwrap_or("unknown"),
                    resource = resource.name(),
                    reason,
                    signoff,
                    "query resource is policy: public"
                );
            }
            #[cfg(debug_assertions)]
            {
                resource.debug_validate_order_coverage()?;
            }
        }
        let public_count = args
            .resources
            .values()
            .filter(|resource| resource.policy_requirement().is_public())
            .count();
        if public_count > 0 {
            tracing::warn!(
                engine = args.engine_name.as_deref().unwrap_or("unknown"),
                public_count,
                "query engine registered public resources; each requires a reason and sign-off"
            );
        }
        let size = size.max(1);
        let engine = args.engine_name.as_deref().unwrap_or("unknown");
        info!(engine, size, "query engine concurrency limit configured");
        Ok(Self {
            args: Arc::new(args),
            concurrency: Arc::new(Semaphore::new(size)),
        })
    }

    /// Scope metas.
    pub async fn scope_metas(&self) -> RiverbaseResult<Vec<QueryRouteMeta>> {
        Ok(self
            .args
            .resources
            .iter()
            .map(|(name, r)| {
                let iface = r.interface();
                let title = iface.title().trim();
                QueryRouteMeta {
                    resource: name.clone(),
                    scope: iface.scope(),
                    title: if title.is_empty() {
                        name.clone()
                    } else {
                        title.to_string()
                    },
                    openapi: iface.openapi(),
                    kind: r.route_kind(),
                    allowed_zones: iface.allowed_zones(),
                }
            })
            .collect())
    }

    /// Back-compat alias for [`Self::scope_metas`] (name + scope only).
    pub async fn http_metas(&self) -> RiverbaseResult<Vec<(String, ScopeMeta)>> {
        Ok(self
            .scope_metas()
            .await?
            .into_iter()
            .map(|m| (m.resource, m.scope))
            .collect())
    }

    /// Execute.
    pub async fn execute(
        &self,
        ctx: &EngineContext,
        query_resource: &str,
        access: QueryAccess,
        request: QueryRequest,
        item_id: Option<&str>,
    ) -> RiverbaseResult<Value> {
        let _permit = self
            .concurrency
            .acquire()
            .await
            .map_err(|_| crate::errors::QRY_006.with_data("semaphore closed"))?;
        run_execute(&self.args, ctx, query_resource, access, request, item_id).await
    }
}

#[async_trait]
impl Engine for QueryEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::Query
    }

    async fn items(&self) -> RiverbaseResult<Vec<String>> {
        Ok(self.args.resources.keys().cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::TenantAccess;
    use uuid::Uuid;

    #[test]
    fn empty_access_is_qry_141() {
        let ctx = EngineContext::new("exp.catalog");
        let err = tenant_access_decision(&ctx, false).expect_err("empty");
        assert_eq!(err.errcode.as_str(), "QRY-141");
    }

    #[test]
    fn exempt_skips_tenant_filter() {
        let ctx = EngineContext::new("exp.catalog");
        assert!(matches!(
            tenant_access_decision(&ctx, true).unwrap(),
            PolicyDecision::Unrestricted
        ));
    }

    #[test]
    fn system_access_skips_tenant_filter() {
        let ctx = EngineContext::new("exp.catalog").with_tenant_access(TenantAccess::All);
        assert!(matches!(
            tenant_access_decision(&ctx, false).unwrap(),
            PolicyDecision::Unrestricted
        ));
    }

    #[test]
    fn tenant_list_uses_in_predicate() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let ctx =
            EngineContext::new("exp.catalog").with_tenant_access(TenantAccess::tenants(vec![a, b]));
        match tenant_access_decision(&ctx, false).unwrap() {
            PolicyDecision::Constrained(expr) => {
                let rendered = format!("{expr:?}");
                assert!(rendered.contains("In"), "{rendered}");
                assert!(rendered.contains(&a.to_string()), "{rendered}");
                assert!(rendered.contains(&b.to_string()), "{rendered}");
            }
            other => panic!("expected constrained, got {other:?}"),
        }
    }
}
