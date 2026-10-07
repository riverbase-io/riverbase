//! Casbin-backed row filter provider that emits bound [`Expr`] values ([SEC-10]).

use riverbase_core::datastore::Expr;
use riverbase_core::query::{PolicyDecision, PolicyFilterProvider, QueryAccess};
use serde_json::Value;

use crate::base::{EngineContext, RiverbaseResult};

/// A claim-bound equality rule derived from the Casbin pack or an explicit table.
#[derive(Debug, Clone)]
pub struct PolicyFilterRule {
    /// Namespace.
    pub namespace: String,
    /// Resource.
    pub resource: String,
    /// Column.
    pub column: String,
    /// Claim key, optionally prefixed with `jwt.`.
    pub claim: String,
}

/// Produces structured [`Expr`] filters; never SQL text.
pub struct CasbinPolicyFilterProvider {
    rules: Vec<PolicyFilterRule>,
}

impl CasbinPolicyFilterProvider {
    /// Construct a new value.
    pub fn new(rules: impl IntoIterator<Item = PolicyFilterRule>) -> Self {
        Self {
            rules: rules.into_iter().collect(),
        }
    }

    /// Parse optional `p_filter, namespace, resource, column, {{claim}}` rows from a policy CSV.
    pub fn from_policy_csv(csv: &str) -> Self {
        let mut rules = Vec::new();
        for line in csv.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split(',').map(str::trim).collect();
            if parts.first().copied() != Some("p_filter") || parts.len() < 5 {
                continue;
            }
            rules.push(PolicyFilterRule {
                namespace: parts[1].to_string(),
                resource: parts[2].to_string(),
                column: parts[3].to_string(),
                claim: normalize_claim_placeholder(parts[4]),
            });
        }
        Self { rules }
    }
}

fn normalize_claim_placeholder(raw: &str) -> String {
    raw.trim()
        .trim_start_matches("{{")
        .trim_end_matches("}}")
        .trim()
        .to_string()
}

fn bound_claim(ctx: &EngineContext, claim: &str) -> Option<String> {
    let key = claim.strip_prefix("jwt.").unwrap_or(claim);
    ctx.jwt_claim_str(key)
        .or_else(|| ctx.claim_str(claim))
        .or_else(|| ctx.claim_str(key))
        .map(str::to_string)
}

impl PolicyFilterProvider for CasbinPolicyFilterProvider {
    fn policy_filter(
        &self,
        ctx: &EngineContext,
        resource: &str,
        _access: QueryAccess,
        _url_scope: Option<&Value>,
    ) -> RiverbaseResult<PolicyDecision> {
        let namespace = ctx.namespace();
        let Some(rule) = self.rules.iter().find(|rule| {
            rule.resource == resource && (rule.namespace.is_empty() || rule.namespace == namespace)
        }) else {
            return Ok(PolicyDecision::Unrestricted);
        };
        let Some(value) = bound_claim(ctx, &rule.claim) else {
            return Err(crate::errors::QRY_124.with_data(serde_json::json!({
                "resource": resource,
                "claim": rule.claim,
            })));
        };
        Ok(PolicyDecision::Constrained(Expr::eq(
            rule.column.clone(),
            value,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riverbase_core::datastore::dsl::PredicateOp;

    #[test]
    fn claim_with_sql_metacharacters_is_bound_not_interpolated() {
        let provider = CasbinPolicyFilterProvider::from_policy_csv(
            "p_filter, exp.catalog, product, organization_id, {{jwt.organization_id}}\n",
        );
        let mut ctx = EngineContext::new("exp.catalog");
        let mut claims = serde_json::Map::new();
        claims.insert(
            "organization_id".into(),
            Value::String("1' OR '1'='1".into()),
        );
        ctx.set_jwt_claims(&claims);
        let decision = provider
            .policy_filter(&ctx, "product", QueryAccess::List, None)
            .expect("filter");
        match decision {
            PolicyDecision::Constrained(Expr::Field { path, op, value }) => {
                assert_eq!(path.0, "organization_id");
                assert_eq!(op, PredicateOp::Eq);
                assert_eq!(value, Value::String("1' OR '1'='1".into()));
            }
            other => panic!("expected constrained expr, got {other:?}"),
        }
    }
}
