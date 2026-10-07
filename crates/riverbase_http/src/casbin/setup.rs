use std::sync::Arc;

use riverbase_core::datastore::PgPool;

use super::authorizer::CasbinActivityAuthorizer;
use super::model::default_activity_model;
use crate::RiverbaseResult;

/// Build a Casbin authorizer from optional inline model/policy text.
///
/// Empty or missing `policy` loads no rules (deny-all until policies are added).
pub async fn build_authorizer(
    pool: PgPool,
    model: Option<&str>,
    policy: Option<&str>,
) -> RiverbaseResult<Arc<CasbinActivityAuthorizer>> {
    let model_text = model
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| default_activity_model());
    let policy_text = policy.filter(|s| !s.trim().is_empty()).unwrap_or("");
    let authorizer =
        CasbinActivityAuthorizer::from_model_and_policy(pool, model_text, policy_text).await?;
    Ok(Arc::new(authorizer))
}
