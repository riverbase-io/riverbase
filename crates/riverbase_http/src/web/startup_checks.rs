//! Startup wiring checks for mounted domains ([DX-05]).

use crate::base::RiverbaseResult;
use crate::domain::DomainQueryEngine;

/// Validate query resources exposed by a mounted domain.
///
/// Checks:
/// - resource names from [`DomainQueryEngine::queries`] are non-empty
/// - [`DomainQueryEngine::query_scope_metas`] resource names are non-empty
///
/// PolicyRequirement / scope_policy consistency is enforced earlier at
/// [`QueryEngine::spawn`](crate::query::QueryEngine::spawn) (`QRY-123` / `QRY-125`).
pub async fn validate_mounted_query_engine(
    namespace: &str,
    query: &dyn DomainQueryEngine,
) -> RiverbaseResult<()> {
    let names = query.queries().await?;
    for name in &names {
        if name.trim().is_empty() {
            return Err(crate::errors::APP_020.with_data(format!("namespace={namespace}")));
        }
    }
    let metas = query.query_scope_metas().await?;
    for meta in metas {
        if meta.resource.trim().is_empty() {
            return Err(crate::errors::APP_021.with_data(format!("namespace={namespace}")));
        }
    }
    Ok(())
}
