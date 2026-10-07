//! Optional Keycloak realm-role gate (`[riverbase.auth] require_realm_role`).

use serde_json::{json, Value};

use super::context::KeycloakTokenPayload;
use crate::base::{RiverbaseError, RiverbaseResult};

/// Deny entry when `required` is set and `realm_access.roles` does not contain it.
///
/// An empty `required` disables the gate. A missing `realm_access` denies when the
/// gate is on.
pub fn require_configured_realm_role(
    auth_user: &KeycloakTokenPayload,
    required: &str,
) -> RiverbaseResult<()> {
    let required = required.trim();
    if required.is_empty() {
        return Ok(());
    }
    let allowed = auth_user
        .realm_access
        .as_ref()
        .and_then(|access| access.get("roles"))
        .and_then(Value::as_array)
        .is_some_and(|roles| roles.iter().any(|role| role.as_str() == Some(required)));
    if allowed {
        Ok(())
    } else {
        Err(realm_role_denied(auth_user, required))
    }
}

fn realm_role_denied(auth_user: &KeycloakTokenPayload, required: &str) -> RiverbaseError {
    crate::errors::AUT_196.with_data(json!({
        "required_realm_role": required,
        "principal": token_principal_errdata(auth_user),
    }))
}

/// Minimal identity from a Keycloak token for AUT-196 `errdata.principal`.
pub fn token_principal_errdata(auth_user: &KeycloakTokenPayload) -> Value {
    json!({
        "sub": auth_user.sub.to_string(),
        "preferred_username": auth_user.preferred_username,
        "email": auth_user.email,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn token(realm_access: Option<Value>) -> KeycloakTokenPayload {
        let mut user: KeycloakTokenPayload = serde_json::from_value(json!({
            "exp": 1,
            "iat": 1,
            "jti": Uuid::nil(),
            "iss": "https://id.example/realms/abx",
            "aud": "abx",
            "sub": Uuid::nil(),
            "sid": Uuid::nil(),
            "preferred_username": "alex",
            "email": "alex@abx.example"
        }))
        .expect("token");
        user.realm_access = realm_access;
        user
    }

    #[test]
    fn empty_requirement_allows_missing_realm_access() {
        assert!(require_configured_realm_role(&token(None), "  ").is_ok());
    }

    #[test]
    fn missing_realm_access_denies_when_required() {
        let denied = require_configured_realm_role(&token(None), "abx-stratify")
            .expect_err("missing realm_access");
        assert_eq!(denied.errcode.as_str(), "AUT-196");
        assert_eq!(denied.errdata["required_realm_role"], "abx-stratify");
        assert_eq!(denied.errdata["principal"]["preferred_username"], "alex");
        assert_eq!(denied.errdata["principal"]["email"], "alex@abx.example");
    }

    #[test]
    fn role_must_be_present() {
        assert!(require_configured_realm_role(
            &token(Some(json!({ "roles": ["abx-stratify", "offline_access"] }))),
            "abx-stratify"
        )
        .is_ok());
        let denied = require_configured_realm_role(
            &token(Some(json!({ "roles": ["offline_access"] }))),
            "abx-stratify",
        )
        .expect_err("wrong role");
        assert_eq!(denied.errcode.as_str(), "AUT-196");
    }
}
