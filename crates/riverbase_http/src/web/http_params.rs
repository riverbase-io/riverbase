use aide::OperationInput;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::base::RiverbaseResult;
use crate::query::FrontendQuery;

/// HTTP query-string adapter. Parameter names and semantics match the Riverbase (Python)
/// `QueryParams` wire contract. See `docs/02-design/query/10-query-engine-design.md` §2.1.
#[derive(Debug, Deserialize, Default, JsonSchema)]
pub struct HttpQueryParams {
    /// Page size (default 25).
    pub limit: Option<u64>,
    /// 1-based page number (default 1).
    pub page: Option<u64>,
    /// CSV of fields to return.
    pub include: Option<String>,
    /// CSV of fields to omit.
    pub exclude: Option<String>,
    /// CSV sort order, e.g. `created.desc,name.asc`.
    pub sort: Option<String>,
    /// User filter as a URL-encoded JSON object.
    pub query: Option<String>,
    /// Base filter as a URL-encoded JSON object (ANDed under `query`).
    pub qbase: Option<String>,
    /// Full-text search term.
    pub text: Option<String>,
    /// When false, list queries skip exact total count (`pagination.total = -1`).
    pub count: Option<bool>,
}

/// Path parameters for unscoped `GET …/{resource}.item/{identifier}`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct QueryItemPath {
    /// Resource identifier.
    pub identifier: String,
}

/// Path parameters for scoped `GET …/{resource}.item/{scope}/{identifier}`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct QueryScopedItemPath {
    /// Scope segment (tenant, organization, etc.).
    pub scope: String,
    /// Resource identifier.
    pub identifier: String,
}

impl OperationInput for HttpQueryParams {}

fn decode_csv(value: Option<String>) -> Option<Vec<String>> {
    let items: Vec<String> = value?
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    (!items.is_empty()).then_some(items)
}

fn decode_json_object(_param: &str, value: Option<String>) -> RiverbaseResult<Option<Value>> {
    let Some(raw) = value else {
        return Ok(None);
    };
    if raw.trim().is_empty() {
        return Ok(None);
    }
    let parsed: Value =
        serde_json::from_str(&raw).map_err(|e| crate::errors::WEB_102.with_data(e.to_string()))?;
    if !parsed.is_object() {
        return Err(crate::errors::WEB_102.with_data(parsed.to_string()));
    }
    Ok(Some(parsed))
}

impl HttpQueryParams {
    /// Convert into frontend query.
    pub fn into_frontend_query(self) -> RiverbaseResult<FrontendQuery> {
        Ok(FrontendQuery {
            limit: self.limit.unwrap_or(25),
            page: self.page.unwrap_or(1).max(1),
            include: decode_csv(self.include),
            exclude: decode_csv(self.exclude),
            sort: decode_csv(self.sort),
            user_query: decode_json_object("query", self.query)?,
            base_query: decode_json_object("qbase", self.qbase)?,
            scope: None,
            text: self.text.filter(|t| !t.is_empty()),
            params: None,
            count: self.count.unwrap_or(true),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_item_path_schema_declares_identifier() {
        let schema = schemars::schema_for!(QueryItemPath);
        let value = serde_json::to_value(&schema).expect("schema json");
        let props = value
            .pointer("/properties/identifier")
            .expect("identifier property");
        assert!(props.is_object());
    }

    #[test]
    fn query_scoped_item_path_schema_declares_scope_and_identifier() {
        let schema = schemars::schema_for!(QueryScopedItemPath);
        let value = serde_json::to_value(&schema).expect("schema json");
        assert!(value.pointer("/properties/scope").is_some());
        assert!(value.pointer("/properties/identifier").is_some());
    }
}
