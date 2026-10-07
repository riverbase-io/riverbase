use std::future::Future;
use std::pin::Pin;

use async_trait::async_trait;
use serde_json::Value;

use super::binding::QueryBinding;
use super::interface::{build_resource_meta, QueryInterface};
use super::primitives::QueryRequest;
use crate::base::{EngineContext, RiverbaseResult, ScopeMeta};
use crate::datastore::dsl::{DataQuery, Expr};
use crate::openapi_meta::OpenApiMeta;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Query Access enumeration.
pub enum QueryAccess {
    /// List.
    List,
    /// Item.
    Item,
    /// Meta.
    Meta,
    /// Report.
    Report,
}

/// Whether a registered resource exposes list/item routes or report routes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryResourceKind {
    /// Query.
    Query,
    /// Report.
    Report,
}

#[derive(Debug, Clone)]
/// Report Output enumeration.
pub enum ReportOutput {
    /// Tabular report body.
    Rows {
        /// Result rows.
        rows: Vec<Value>,
        /// Total matching rows (`-1` when uncounted).
        total: i64,
    },
    /// Document.
    Document(Value),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
/// Policy Requirement enumeration.
pub enum PolicyRequirement {
    /// `scope_policy` must produce a constraint; unrestricted access is denied ([SEC-01]).
    #[default]
    Required,
    /// Legacy: unrestricted `scope_policy` results are allowed (prefer `Public`).
    Optional,
    /// Explicitly public resource; unrestricted access is permitted ([SEC-01]).
    Public {
        /// Reason.
        reason: &'static str,
        /// Signoff.
        signoff: &'static str,
    },
}

impl PolicyRequirement {
    /// Whether this is required.
    pub fn is_required(self) -> bool {
        matches!(self, Self::Required)
    }

    /// Whether this is public.
    pub fn is_public(self) -> bool {
        matches!(self, Self::Public { .. })
    }

    /// Public reason.
    pub fn public_reason(self) -> Option<&'static str> {
        match self {
            Self::Public { reason, .. } => Some(reason),
            _ => None,
        }
    }

    /// Public signoff.
    pub fn public_signoff(self) -> Option<&'static str> {
        match self {
            Self::Public { signoff, .. } => Some(signoff),
            _ => None,
        }
    }
}

/// Explicit query-policy result. `Unrestricted` is intentionally distinct from `Denied`.
#[derive(Debug, Clone)]
pub enum PolicyDecision {
    /// Unrestricted.
    Unrestricted,
    /// Constrained.
    Constrained(Expr),
    /// Denied.
    Denied(Value),
}

impl PolicyDecision {
    /// Combine.
    pub fn combine(self, other: Self) -> Self {
        match (self, other) {
            (Self::Denied(reason), _) | (_, Self::Denied(reason)) => Self::Denied(reason),
            (Self::Unrestricted, decision) | (decision, Self::Unrestricted) => decision,
            (Self::Constrained(left), Self::Constrained(right)) => {
                Self::Constrained(Expr::And(vec![left, right]))
            }
        }
    }

    /// Convert into filter.
    pub fn into_filter(self, resource: &str) -> RiverbaseResult<Option<Expr>> {
        match self {
            Self::Unrestricted => Ok(None),
            Self::Constrained(filter) => Ok(Some(filter)),
            Self::Denied(reason) => Err(crate::errors::QRY_121
                .with_data(serde_json::json!({ "resource": resource, "reason": reason }))),
        }
    }
}

impl From<Option<Expr>> for PolicyDecision {
    fn from(value: Option<Expr>) -> Self {
        match value {
            Some(filter) => Self::Constrained(filter),
            None => Self::Unrestricted,
        }
    }
}

/// HTTP routing metadata for a query resource (reserved for HTTP-only knobs; scope lives on [`QueryInterface::scope`]).
#[derive(Debug, Clone, Default)]
pub struct QueryHttpMeta {}

/// Per-resource HTTP route registration metadata (name, scope, OpenAPI title).
#[derive(Debug, Clone)]
pub struct QueryRouteMeta {
    /// Resource.
    pub resource: String,
    /// Scope.
    pub scope: ScopeMeta,
    /// Title.
    pub title: String,
    /// Openapi.
    pub openapi: OpenApiMeta,
    /// Kind.
    pub kind: QueryResourceKind,
    /// Whitelist of deployment zones where this query/report route is registered.
    pub allowed_zones: Vec<String>,
}

impl QueryRouteMeta {
    /// Whether this query/report route should be registered for the configured deployment zones.
    pub fn zone_allowed(&self, api_zone: &[String]) -> bool {
        crate::util::zone_allowed(&self.allowed_zones, api_zone)
    }
}

/// A queryable resource: a frontend [`QueryInterface`], a backend [`QueryBinding`], and an
/// executor that runs a lowered [`DataQuery`]. Lowering, projection/relabel, and the
/// response envelope are handled centrally by the [`QueryEngine`](super::engine::QueryEngine).
#[async_trait]
pub trait QueryResource: Send + Sync {
    /// Name.
    fn name(&self) -> &str;

    /// Frontend contract (logical fields, presets, `.meta`).
    fn interface(&self) -> &dyn QueryInterface;

    /// Backend schema mapping (logical field → physical column, joins, coercion).
    fn binding(&self) -> &dyn QueryBinding;

    /// Execute a lowered list query, returning rows keyed by **physical** column names.
    async fn execute_list(&self, query: &DataQuery) -> RiverbaseResult<Vec<Value>>;

    /// Like [`Self::execute_list`] but also returns total matching rows (`-1` if unknown).
    async fn execute_list_with_total(&self, query: &DataQuery) -> RiverbaseResult<(Vec<Value>, i64)> {
        Ok((self.execute_list(query).await?, -1))
    }

    /// First-class report hook with validated request params and execution context.
    fn execute_report<'a>(
        &'a self,
        ctx: &'a EngineContext,
        session: &'a super::session::QuerySession,
        _request: &'a QueryRequest,
        query: &'a DataQuery,
    ) -> Pin<Box<dyn Future<Output = RiverbaseResult<ReportOutput>> + Send + 'a>> {
        let _ = (ctx, session);
        Box::pin(async move {
            let (rows, total) = self.execute_list_with_total(query).await?;
            Ok(ReportOutput::Rows { rows, total })
        })
    }

    /// Execute a lowered item query by identifier (physical-keyed row, or `None`).
    async fn execute_item(&self, query: &DataQuery, id: &str) -> RiverbaseResult<Option<Value>>;

    /// Error code returned when an item query finds no matching row.
    fn item_not_found_error_code(&self) -> &'static str {
        "QRY-101"
    }

    /// Server-side scope policy: derive a restriction (in **physical** columns) from the engine
    /// context and the decoded URL scope. The result is ANDed into [`DataQuery::policy_filter`]
    /// ahead of the client filter and is **not** validated against the public interface.
    ///
    /// Declared via the `scope_policy(ctx, url_scope) { … }` block in `query_resource!` /
    /// `report_resource!`; defaults to no restriction.
    fn scope_policy(
        &self,
        _ctx: &EngineContext,
        _url_scope: Option<&Value>,
    ) -> RiverbaseResult<Option<Expr>> {
        Ok(None)
    }

    /// Explicit policy decision used by the query engine.
    fn policy_decision(
        &self,
        ctx: &EngineContext,
        url_scope: Option<&Value>,
    ) -> RiverbaseResult<PolicyDecision> {
        self.scope_policy(ctx, url_scope).map(PolicyDecision::from)
    }

    /// Policy requirement.
    fn policy_requirement(&self) -> PolicyRequirement {
        PolicyRequirement::Required
    }

    /// Whether this resource declared an explicit `scope_policy` block.
    fn has_scope_policy(&self) -> bool {
        false
    }

    /// Whether this resource is exempt from automatic `_tenant` row filtering.
    fn tenant_scope_exempt(&self) -> bool {
        false
    }

    /// Policy filter enforced.
    fn policy_filter_enforced(&self) -> bool {
        false
    }

    /// Http meta.
    fn http_meta(&self) -> QueryHttpMeta {
        QueryHttpMeta::default()
    }

    /// Which HTTP routes this resource exposes (list/item vs report).
    fn route_kind(&self) -> QueryResourceKind {
        QueryResourceKind::Query
    }

    /// Resource descriptor (§7.1), built from the interface and binding.
    fn meta(&self) -> Value {
        build_resource_meta(self.name(), self.interface(), Some(self.binding()))
    }

    /// Validate sortable / default_order fields against entity orderable columns (debug builds only).
    fn debug_validate_order_coverage(&self) -> RiverbaseResult<()> {
        Ok(())
    }
}
