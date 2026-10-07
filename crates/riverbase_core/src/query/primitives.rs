//! Native query primitives (no HTTP concerns): the normalized [`FrontendQuery`].

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Sort Direction enumeration.
pub enum SortDirection {
    /// Asc.
    Asc,
    /// Desc.
    Desc,
}

impl SortDirection {
    /// Borrow as r.
    pub fn as_str(self) -> &'static str {
        match self {
            SortDirection::Asc => "asc",
            SortDirection::Desc => "desc",
        }
    }
}

/// Normalized, structured representation of a client's query intent.
///
/// Field names and semantics match the Riverbase (Python) `QueryParams` wire contract.
/// See `docs/02-design/query/10-query-engine-design.md` §2.6.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct FrontendQuery {
    /// Page size; max items returned.
    pub limit: u64,
    /// 1-based page number. `offset = (page - 1) * limit`.
    pub page: u64,
    /// Fields to return; empty/`None` selects all selectable fields.
    pub include: Option<Vec<String>>,
    /// Fields to omit from the result.
    pub exclude: Option<Vec<String>>,
    /// Sort order entries, e.g. `["created.desc", "name.asc"]`.
    pub sort: Option<Vec<String>>,
    /// User filter (`query`), applied on top of `base_query`.
    pub user_query: Option<Value>,
    /// Base filter (`qbase`); ANDed with `user_query` (user can only narrow it).
    pub base_query: Option<Value>,
    /// Decoded scope from the URL path (when the resource requires scoping).
    pub scope: Option<Value>,
    /// Full-text search term; honored only when the resource allows it.
    pub text: Option<String>,
    /// Report input parameters (declarative; not lowered into the query automatically).
    pub params: Option<Value>,
    /// When false, list queries omit exact total count (`pagination.total = null` / `-1` sentinel).
    pub count: bool,
}

impl Default for FrontendQuery {
    fn default() -> Self {
        Self {
            limit: 25,
            page: 1,
            include: None,
            exclude: None,
            sort: None,
            user_query: None,
            base_query: None,
            scope: None,
            text: None,
            params: None,
            count: true,
        }
    }
}

impl FrontendQuery {
    /// 0-based offset derived from the 1-based `page`.
    pub fn offset(&self) -> u64 {
        self.page.saturating_sub(1).saturating_mul(self.limit)
    }
}

/// Back-compat alias: the structured request threaded through the engine/transport.
pub type QueryRequest = FrontendQuery;
