use std::collections::HashMap;

use oauth2::{CsrfToken, PkceCodeChallenge};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;

use super::config::AuthConfig;
use super::session_helper::uri;
use crate::base::RiverbaseResult;

const OAUTH_PKCE_VERIFIER: &str = "oauth_pkce_verifier";
const OAUTH_STATE: &str = "oauth_state";
const OAUTH_REDIRECT_URI: &str = "oauth_redirect_uri";

/// Exchanged OAuth2 token bundle.
#[derive(Debug, Clone, Deserialize)]
pub struct OAuthTokenBundle {
    #[serde(default)]
    pub access_token: Option<String>,
    #[serde(default)]
    pub id_token: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub expires_in: Option<u64>,
}

/// Keycloak OAuth2 authorization-code client (PKCE).
#[derive(Clone)]
pub struct KeycloakOAuth {
    config: AuthConfig,
    http: Client,
    issuer: String,
    token_url: String,
}

fn is_gitea_issuer(issuer: &str) -> bool {
    !issuer.contains("/realms/") && !issuer.contains("/protocol/openid-connect")
}

impl KeycloakOAuth {
    /// Construct a new value.
    pub fn new(config: AuthConfig) -> RiverbaseResult<Self> {
        let issuer = config.effective_issuer();
        let token_url = if is_gitea_issuer(&issuer) {
            crate::auth::rewrite_internal_oidc_url(&uri(
                &issuer,
                &["login", "oauth", "access_token"],
                None,
            ))
        } else {
            uri(&issuer, &["protocol", "openid-connect", "token"], None)
        };

        let timeout = std::time::Duration::from_secs(config.http_timeout_secs);
        let http = Client::builder().timeout(timeout).build().map_err(|e| {
            crate::errors::AUT_186.with_data(serde_json::json!({ "detail": e.to_string() }))
        })?;

        Ok(Self {
            config,
            http,
            issuer,
            token_url,
        })
    }

    /// Issuer.
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// Signup url.
    pub fn signup_url(&self) -> String {
        uri(
            &self.issuer,
            &["protocol", "openid-connect", "registrations"],
            None,
        )
    }

    /// Logout url.
    pub fn logout_url(&self) -> String {
        uri(
            &self.issuer,
            &["protocol", "openid-connect", "logout"],
            None,
        )
    }

    /// Begin authorization: returns (authorize_url, pkce_verifier, csrf_state).
    pub fn authorize_url(&self, redirect_uri: &str) -> RiverbaseResult<(String, String, String)> {
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        let csrf_state = CsrfToken::new_random();

        let mut q: HashMap<String, String> = HashMap::new();
        q.insert("client_id".into(), self.config.oauth2_client_id.clone());
        q.insert("redirect_uri".into(), redirect_uri.to_string());
        q.insert("response_type".into(), "code".into());
        q.insert(
            "scope".into(),
            if is_gitea_issuer(&self.issuer) {
                "openid profile email groups".into()
            } else {
                "openid profile email".into()
            },
        );
        q.insert("state".into(), csrf_state.secret().to_string());
        q.insert("code_challenge".into(), pkce_challenge.as_str().to_string());
        q.insert("code_challenge_method".into(), "S256".into());

        let auth_path: &[&str] = if is_gitea_issuer(&self.issuer) {
            &["login", "oauth", "authorize"]
        } else {
            &["protocol", "openid-connect", "auth"]
        };
        let url = uri(&self.issuer, auth_path, Some(&q));

        Ok((
            url,
            pkce_verifier.secret().to_string(),
            csrf_state.secret().to_string(),
        ))
    }

    /// Session keys.
    pub fn session_keys() -> (&'static str, &'static str) {
        (OAUTH_PKCE_VERIFIER, OAUTH_STATE)
    }

    /// Redirect uri session key.
    pub fn redirect_uri_session_key() -> &'static str {
        OAUTH_REDIRECT_URI
    }

    /// Exchange authorization code for tokens (validates CSRF state when stored).
    pub async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
        pkce_verifier: &str,
        expected_state: Option<&str>,
        received_state: Option<&str>,
    ) -> RiverbaseResult<OAuthTokenBundle> {
        if let (Some(expected), Some(received)) = (expected_state, received_state) {
            if expected != received {
                return Err(crate::errors::AUT_150.with_data(serde_json::json!({})));
            }
        }

        let mut form: Vec<(&str, &str)> = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", &self.config.oauth2_client_id),
            ("code_verifier", pkce_verifier),
        ];
        let secret = self.config.oauth2_client_secret.as_str();
        if !secret.is_empty() {
            form.push(("client_secret", secret));
        }

        let response = self
            .http
            .post(&self.token_url)
            .form(&form)
            .send()
            .await
            .map_err(|e| {
                crate::errors::AUT_151.with_data(serde_json::json!({ "detail": e.to_string() }))
            })?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(crate::errors::AUT_151.with_data(serde_json::json!({ "detail": body })));
        }

        response.json::<OAuthTokenBundle>().await.map_err(|e| {
            crate::errors::AUT_151.with_data(serde_json::json!({ "detail": e.to_string() }))
        })
    }

    /// Build Keycloak logout redirect URL with id_token_hint.
    pub fn logout_redirect(&self, id_token: &str, post_logout_redirect_uri: &str) -> String {
        let mut q: HashMap<String, String> = HashMap::new();
        q.insert("id_token_hint".into(), id_token.to_string());
        q.insert(
            "post_logout_redirect_uri".into(),
            post_logout_redirect_uri.to_string(),
        );
        uri(&self.logout_url(), &[], Some(&q))
    }
}

/// Merge realm_access/resource_access from access token into ID token claims.
pub fn merge_token_claims(id_data: &mut Value, ac_data: &Value) {
    if let Some(obj) = id_data.as_object_mut() {
        if let Some(ra) = ac_data.get("realm_access") {
            obj.insert("realm_access".into(), ra.clone());
        }
        if let Some(ra) = ac_data.get("resource_access") {
            obj.insert("resource_access".into(), ra.clone());
        }
    }
}
