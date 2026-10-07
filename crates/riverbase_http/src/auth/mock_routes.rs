//! Browser routes for [`AuthProvider::MockAuth`] (local OAuth2 IdP).

use std::sync::Arc;

use aide::axum::{routing::get_with, ApiRouter};
use aide::NoApi;
use axum::{
    extract::{Form, Query},
    http::HeaderMap,
    middleware,
    response::{Html, IntoResponse, Redirect, Response},
    Extension, Json,
};
use axum_extra::extract::CookieJar;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use tower_sessions::Session;

use super::callback_error::{callback_failure_response, negotiate_auth_error_html};
use super::context::{AuthorizationContext, SessionOrganization, SessionProfile};
use super::csrf::generate_csrf_token;
use super::mock_idp::{
    mint_id_token, mock_authorize_url, mock_callback_redirect, mock_password_ok,
    pkce_challenge_s256, pkce_matches, render_picker_html, MockIdp,
};
use super::oauth::KeycloakOAuth;
use super::provider::AuthProfileProvider;
use super::routes::{auth_op, sign_out_redirect_target, SignInQuery, SignOutQuery};
use super::session_establish::{establish_browser_session, id_token_removal_cookie};
use super::session_helper::{
    auth_home_location, prefix_from_callback_uri, public_api_path, validate_redirect_url,
};
use crate::base::RiverbaseError;
use crate::config::AuthConfig;
use crate::web::openapi::RiverbaseOperationKind;

/// Post-login path stored when `/auth/sign-in` has no `next`.
///
/// Uses the configured sign-in URI as-is. An empty value stays on `/`.
fn sign_in_landing(configured: &str) -> String {
    let configured = configured.trim();
    if configured.is_empty() {
        "/".into()
    } else {
        configured.to_string()
    }
}

/// State for mock-auth HTTP routes (local OAuth2 IdP).
#[derive(Clone)]
pub struct MockAuthRouteState {
    /// Config.
    pub config: Arc<AuthConfig>,
    /// Profile provider.
    pub profile_provider: Arc<dyn AuthProfileProvider>,
    /// In-memory authorization codes.
    pub idp: Arc<MockIdp>,
}

/// Browser OAuth2 routes for [`AuthProvider::MockAuth`].
pub fn mock_auth_router(
    config: Arc<AuthConfig>,
    profile_provider: Arc<dyn AuthProfileProvider>,
) -> ApiRouter {
    let base = config.base_path.trim_end_matches('/').to_string();
    let home = format!("{base}/");
    let verify = format!("{base}/verify");
    let info = format!("{base}/info");
    let csrf = format!("{base}/csrf-token");
    let path_home = format!("{base}/home");
    let path_sign_in = format!("{base}/sign-in");
    let path_sign_out = format!("{base}/sign-out");
    let path_mock_auth = format!("{base}/mock-auth");
    let path_callback = format!("{base}/callback");

    let state = MockAuthRouteState {
        config,
        profile_provider,
        idp: Arc::new(MockIdp::new()),
    };
    let html_config = state.config.as_ref().clone();
    ApiRouter::new()
        .api_route(
            home.as_str(),
            get_with(mock_auth_home, |op| {
                auth_op(
                    "mock_auth_home",
                    "Mock auth home",
                    "Mock authentication entry point.",
                    "",
                    RiverbaseOperationKind::GenericGet,
                )(op)
                .response_with::<200, Json<Value>, _>(|res| {
                    res.description("Mock auth entry message")
                })
            }),
        )
        .api_route(
            verify.as_str(),
            get_with(mock_auth_verify, |op| {
                auth_op(
                    "mock_auth_verify",
                    "Mock verify session",
                    "Return mock authenticated session details.",
                    "verify",
                    RiverbaseOperationKind::GenericGet,
                )(op)
                .response_with::<200, Json<Value>, _>(|res| {
                    res.description("Mock session verification payload")
                })
            }),
        )
        .api_route(
            info.as_str(),
            get_with(mock_auth_info, |op| {
                auth_op(
                    "mock_auth_info",
                    "Mock auth info",
                    "Return mock authorization context.",
                    "info",
                    RiverbaseOperationKind::GenericGet,
                )(op)
                .response_with::<200, Json<AuthorizationContext>, _>(|res| {
                    res.description("Mock authorization context")
                })
            }),
        )
        .api_route(
            csrf.as_str(),
            get_with(mock_get_csrf_token, |op| {
                auth_op(
                    "mock_auth_csrf_token",
                    "Mock CSRF token",
                    "Return a CSRF token for mock auth flows.",
                    "csrf-token",
                    RiverbaseOperationKind::GenericGet,
                )(op)
                .response_with::<200, Json<Value>, _>(|res| res.description("CSRF token payload"))
            }),
        )
        .api_route(
            path_home.as_str(),
            get_with(
                mock_auth_session_home,
                auth_op(
                    "mock_auth_session_home",
                    "Mock auth session home",
                    "Redirect to the SPA when the mock session is valid, otherwise start the IdP.",
                    "home",
                    RiverbaseOperationKind::GenericGet,
                ),
            ),
        )
        .api_route(
            path_sign_in.as_str(),
            get_with(
                mock_sign_in,
                auth_op(
                    "mock_auth_sign_in",
                    "Mock sign in",
                    "Start the MockAuth authorization-code flow (redirects to the user picker).",
                    "sign-in",
                    RiverbaseOperationKind::GenericGet,
                ),
            ),
        )
        .route(
            path_mock_auth.as_str(),
            axum::routing::get(mock_picker_get).post(mock_picker_post),
        )
        .api_route(
            path_callback.as_str(),
            get_with(
                mock_oauth_callback,
                auth_op(
                    "mock_auth_callback",
                    "Mock OAuth callback",
                    "Exchange the mock authorization code and establish the browser session.",
                    "callback",
                    RiverbaseOperationKind::GenericGet,
                ),
            ),
        )
        .api_route(
            path_sign_out.as_str(),
            get_with(
                mock_sign_out,
                auth_op(
                    "mock_auth_sign_out",
                    "Mock sign out",
                    "End the MockAuth session and redirect to the landing page.",
                    "sign-out",
                    RiverbaseOperationKind::GenericGet,
                ),
            ),
        )
        .layer(Extension(Arc::new(state)))
        .layer(middleware::from_fn_with_state(html_config, negotiate_auth_error_html))
}

async fn mock_auth_home(Extension(state): Extension<Arc<MockAuthRouteState>>) -> Json<Value> {
    let base = state.config.base_path.trim_end_matches('/');
    Json(json!({
        "message": format!("Go to {base}/sign-in to start MockAuth OAuth2 login"),
        "auth_provider": "MockAuth"
    }))
}

async fn mock_session_user(session: &Session, config: &AuthConfig) -> Option<Value> {
    session
        .get::<Value>(&config.ses_user_field)
        .await
        .ok()
        .flatten()
}

async fn mock_auth_info(
    Extension(state): Extension<Arc<MockAuthRouteState>>,
    NoApi(session): NoApi<Session>,
    headers: HeaderMap,
) -> Result<Json<AuthorizationContext>, RiverbaseError> {
    let session_user = mock_session_user(&session, &state.config).await;
    Ok(Json(mock_authorization_context_from_headers(
        &state.config,
        headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok()),
        session_user.as_ref(),
    )?))
}

async fn mock_auth_verify(
    Extension(state): Extension<Arc<MockAuthRouteState>>,
    NoApi(session): NoApi<Session>,
    headers: HeaderMap,
) -> Result<Json<Value>, RiverbaseError> {
    let session_user = mock_session_user(&session, &state.config).await;
    let ctx = mock_authorization_context_from_headers(
        &state.config,
        headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok()),
        session_user.as_ref(),
    )?;
    Ok(Json(json!({
        "status": "OK",
        "message": "User logged in (mock).",
        "context": ctx,
        "headers": { "cookie": "<redacted>" }
    })))
}

async fn mock_get_csrf_token(NoApi(session): NoApi<Session>) -> Result<Json<Value>, RiverbaseError> {
    let token: String = match session.get("csrf_token").await.ok().flatten() {
        Some(t) => t,
        None => {
            let t = generate_csrf_token();
            session.insert("csrf_token", t.clone()).await.map_err(|e| {
                crate::errors::AUT_159.with_data(json!({ "detail": e.to_string() }))
            })?;
            t
        }
    };
    Ok(Json(json!({ "csrf_token": token })))
}

async fn mock_auth_session_home(
    Extension(state): Extension<Arc<MockAuthRouteState>>,
    NoApi(session): NoApi<Session>,
    headers: HeaderMap,
    Query(params): Query<SignInQuery>,
) -> Result<Redirect, RiverbaseError> {
    let session_user = mock_session_user(&session, &state.config).await;
    let authenticated = mock_authorization_context_from_headers(
        &state.config,
        headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok()),
        session_user.as_ref(),
    )
    .is_ok();
    if authenticated {
        return Ok(Redirect::to(&auth_home_location(
            &state.config,
            params.next.as_deref(),
        )));
    }
    start_mock_sign_in(&state, &session, &params, &headers).await
}

/// Store PKCE/state and redirect to the mock IdP. Shared by `/auth/home` (anonymous) and `/auth/sign-in`.
async fn start_mock_sign_in(
    state: &MockAuthRouteState,
    session: &Session,
    params: &SignInQuery,
    headers: &HeaderMap,
) -> Result<Redirect, RiverbaseError> {
    let csrf_token = generate_csrf_token();
    session
        .insert("csrf_token", csrf_token)
        .await
        .map_err(|e| crate::errors::AUT_159.with_data(json!({ "detail": e.to_string() })))?;
    let fallback = prefix_from_callback_uri(&state.config.default_callback_uri);
    let next = params
        .next
        .as_deref()
        .filter(|s| s.starts_with('/') && !s.starts_with("//"))
        .map(|path| public_api_path(path, headers, &fallback))
        .unwrap_or_else(|| sign_in_landing(&state.config.default_signin_redirect_uri));
    session
        .insert("next", next)
        .await
        .map_err(|e| crate::errors::AUT_159.with_data(json!({ "detail": e.to_string() })))?;

    let callback_uri = validate_redirect_url(
        params.callback.as_deref().unwrap_or(""),
        &state.config.default_callback_uri,
        &state.config.safe_redirect_domains,
        false,
    );
    let auth_base = public_api_path(
        state.config.base_path.trim_end_matches('/'),
        headers,
        &fallback,
    );
    let (url, pkce_verifier, oauth_state, _challenge) = mock_authorize_url(
        &auth_base,
        state.config.oauth2_client_id.trim(),
        &callback_uri,
    );
    let (pkce_key, state_key) = KeycloakOAuth::session_keys();
    session
        .insert(pkce_key, pkce_verifier)
        .await
        .map_err(|e| crate::errors::AUT_159.with_data(json!({ "detail": e.to_string() })))?;
    session
        .insert(state_key, oauth_state)
        .await
        .map_err(|e| crate::errors::AUT_159.with_data(json!({ "detail": e.to_string() })))?;
    session
        .insert(KeycloakOAuth::redirect_uri_session_key(), callback_uri)
        .await
        .map_err(|e| crate::errors::AUT_159.with_data(json!({ "detail": e.to_string() })))?;
    Ok(Redirect::to(&url))
}

async fn mock_sign_in(
    Extension(state): Extension<Arc<MockAuthRouteState>>,
    NoApi(session): NoApi<Session>,
    Query(params): Query<SignInQuery>,
    headers: HeaderMap,
) -> Result<Redirect, RiverbaseError> {
    start_mock_sign_in(&state, &session, &params, &headers).await
}

#[derive(Debug, Deserialize, JsonSchema)]
struct MockAuthorizeQuery {
    state: Option<String>,
    code_challenge: Option<String>,
    redirect_uri: Option<String>,
    client_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct MockPickerForm {
    sub: String,
    #[serde(default)]
    password: String,
    state: Option<String>,
    code_challenge: Option<String>,
    redirect_uri: Option<String>,
    client_id: Option<String>,
}

async fn mock_picker_get(
    Extension(state): Extension<Arc<MockAuthRouteState>>,
    session: Session,
    Query(params): Query<MockAuthorizeQuery>,
    headers: HeaderMap,
) -> Result<Html<String>, RiverbaseError> {
    let (pkce_key, state_key) = KeycloakOAuth::session_keys();
    let stored_state: Option<String> = session.get(state_key).await.ok().flatten();
    let stored_redirect: Option<String> = session
        .get(KeycloakOAuth::redirect_uri_session_key())
        .await
        .ok()
        .flatten();
    let stored_verifier: Option<String> = session.get(pkce_key).await.ok().flatten();
    let oauth_state = params.state.or(stored_state).ok_or_else(|| {
        crate::errors::AUT_153
            .with_data(json!({ "detail": "Missing OAuth state. Start from /auth/sign-in." }))
    })?;
    let redirect_uri = params
        .redirect_uri
        .or(stored_redirect)
        .unwrap_or_else(|| state.config.default_callback_uri.clone());
    let challenge = params
        .code_challenge
        .clone()
        .or_else(|| stored_verifier.as_deref().map(pkce_challenge_s256))
        .unwrap_or_default();
    let client_id = params
        .client_id
        .unwrap_or_else(|| state.config.oauth2_client_id.clone());
    let action = mock_auth_action(&state.config, &headers);
    Ok(Html(render_picker_html(
        &action,
        &state.config.all_mock_users(),
        &oauth_state,
        &challenge,
        &redirect_uri,
        &client_id,
        None,
    )))
}

fn mock_auth_action(config: &AuthConfig, headers: &HeaderMap) -> String {
    let fallback = prefix_from_callback_uri(&config.default_callback_uri);
    format!(
        "{}/mock-auth",
        public_api_path(config.base_path.trim_end_matches('/'), headers, &fallback,)
    )
}

async fn mock_picker_post(
    Extension(state): Extension<Arc<MockAuthRouteState>>,
    session: Session,
    headers: HeaderMap,
    Form(form): Form<MockPickerForm>,
) -> Result<Response, RiverbaseError> {
    let user = state
        .config
        .mock_user_by_sub(form.sub.trim())
        .ok_or_else(|| {
            crate::errors::AUT_153.with_data(json!({ "detail": "Unknown mock user." }))
        })?;
    let (pkce_key, state_key) = KeycloakOAuth::session_keys();
    let stored_state: Option<String> = session.get(state_key).await.ok().flatten();
    let oauth_state = form.state.or(stored_state).ok_or_else(|| {
        crate::errors::AUT_153.with_data(json!({ "detail": "Missing OAuth state." }))
    })?;
    let redirect_uri = form
        .redirect_uri
        .or(session
            .get(KeycloakOAuth::redirect_uri_session_key())
            .await
            .ok()
            .flatten())
        .unwrap_or_else(|| state.config.default_callback_uri.clone());
    let challenge = match form.code_challenge.filter(|s| !s.is_empty()) {
        Some(value) => value,
        None => {
            let verifier: Option<String> = session.get(pkce_key).await.ok().flatten();
            verifier
                .as_deref()
                .map(pkce_challenge_s256)
                .ok_or_else(|| {
                    crate::errors::AUT_153.with_data(json!({ "detail": "Missing PKCE challenge." }))
                })?
        }
    };
    let expected_client = state.config.oauth2_client_id.trim();
    let client_id = form
        .client_id
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| expected_client.to_string());
    if !expected_client.is_empty() && client_id != expected_client {
        return Err(crate::errors::AUT_153.with_data(json!({
            "detail": "client_id does not match this application.",
        })));
    }
    if let Err(message) = mock_password_ok(&user, &form.password) {
        return Ok(Html(render_picker_html(
            &mock_auth_action(&state.config, &headers),
            &state.config.all_mock_users(),
            &oauth_state,
            &challenge,
            &redirect_uri,
            &client_id,
            Some(message),
        ))
        .into_response());
    }
    let (code, session_state) =
        state
            .idp
            .issue_code(&user.sub, &challenge, expected_client, &redirect_uri);
    Ok(Redirect::to(&mock_callback_redirect(
        &redirect_uri,
        &code,
        &oauth_state,
        &session_state,
    ))
    .into_response())
}

#[derive(Debug, Deserialize, JsonSchema)]
struct MockCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    #[allow(dead_code)]
    session_state: Option<String>,
    error: Option<String>,
}

async fn mock_oauth_callback(
    Extension(state): Extension<Arc<MockAuthRouteState>>,
    NoApi(session): NoApi<Session>,
    NoApi(jar): NoApi<CookieJar>,
    headers: HeaderMap,
    Query(params): Query<MockCallbackQuery>,
) -> Response {
    match mock_oauth_callback_result(&state, &session, params).await {
        Ok(response) => response,
        Err(err) => callback_failure_response(&session, &state.config, jar, &headers, err).await,
    }
}

async fn mock_oauth_callback_result(
    state: &MockAuthRouteState,
    session: &Session,
    params: MockCallbackQuery,
) -> Result<Response, RiverbaseError> {
    if params.error.is_some() {
        return Err(crate::errors::AUT_152.with_data(json!({})));
    }
    let code = params.code.ok_or_else(|| {
        crate::errors::AUT_153.with_data(json!({ "detail": "Missing authorization code" }))
    })?;
    let (pkce_key, state_key) = KeycloakOAuth::session_keys();
    let pkce_verifier: Option<String> = session.get(pkce_key).await.ok().flatten();
    let expected_state: Option<String> = session.get(state_key).await.ok().flatten();
    let pkce_verifier = pkce_verifier.ok_or_else(|| crate::errors::AUT_190.with_data(json!({})))?;
    if let (Some(expected), Some(received)) = (expected_state.as_deref(), params.state.as_deref()) {
        if expected != received {
            return Err(crate::errors::AUT_150.with_data(json!({})));
        }
    }
    let pending = state.idp.consume_code(&code).ok_or_else(|| {
        crate::errors::AUT_153.with_data(json!({
            "detail": "Authorization code is invalid or expired."
        }))
    })?;
    if !pkce_matches(&pkce_verifier, &pending.code_challenge) {
        return Err(crate::errors::AUT_191.with_data(json!({})));
    }
    let user = state.config.mock_user_by_sub(&pending.sub).ok_or_else(|| {
        crate::errors::AUT_153.with_data(json!({ "detail": "Unknown mock user." }))
    })?;
    let (id_token, claims) = mint_id_token(&state.config, &user, &pending.session_state)?;
    establish_browser_session(
        session,
        &state.config,
        state.profile_provider.as_ref(),
        claims,
        &id_token,
    )
    .await
}

async fn mock_sign_out(
    Extension(state): Extension<Arc<MockAuthRouteState>>,
    NoApi(session): NoApi<Session>,
    NoApi(jar): NoApi<CookieJar>,
    Query(params): Query<SignOutQuery>,
    headers: HeaderMap,
) -> Result<Response, RiverbaseError> {
    session
        .flush()
        .await
        .map_err(|e| crate::errors::AUT_159.with_data(json!({ "detail": e.to_string() })))?;
    let fallback = {
        let dest = state.config.default_logout_redirect_uri.trim();
        if dest.is_empty() {
            "/"
        } else {
            dest
        }
    };
    let dest = sign_out_redirect_target(
        &params,
        &headers,
        fallback,
        &state.config.safe_redirect_domains,
        true,
    );
    let out_jar = jar.remove(id_token_removal_cookie(&state.config));
    Ok((out_jar, Redirect::to(&dest)).into_response())
}

pub(crate) fn mock_authorization_context_from_headers(
    config: &AuthConfig,
    authorization: Option<&str>,
    session_user: Option<&Value>,
) -> Result<AuthorizationContext, RiverbaseError> {
    if let Some(header) = authorization.map(str::trim).filter(|s| !s.is_empty()) {
        let principal = crate::auth::resolve_mock_principal(Some(header))?
            .ok_or_else(|| crate::errors::AUT_193.with_data(json!({})))?;
        return mock_authorization_context(config, &principal);
    }
    if let Some(claims) = session_user {
        let principal = crate::auth::principal_from_mock_claims(claims)?;
        return mock_authorization_context(config, &principal);
    }
    Err(crate::errors::AUT_195.with_data(json!({})))
}

fn mock_authorization_context(
    config: &AuthConfig,
    principal: &crate::auth::Principal,
) -> Result<AuthorizationContext, RiverbaseError> {
    let profile_id = uuid::Uuid::parse_str(&principal.sub).unwrap_or(uuid::Uuid::nil());
    let org_claim = claim_string(&principal.claims, "org_id")
        .or_else(|| claim_string(&principal.claims, "organization_id"));
    let org_id = org_claim
        .as_deref()
        .and_then(|s| s.parse::<uuid::Uuid>().ok())
        .filter(|id| !id.is_nil())
        .or_else(|| {
            config
                .mock_org_id
                .trim()
                .parse::<uuid::Uuid>()
                .ok()
                .filter(|id| !id.is_nil())
        })
        .or_else(|| (!profile_id.is_nil()).then_some(profile_id));
    let given_name = claim_string(&principal.claims, "given_name")
        .or_else(|| config.mock_given_name.clone().filter(|s| !s.is_empty()));
    let family_name = claim_string(&principal.claims, "family_name")
        .or_else(|| config.mock_family_name.clone().filter(|s| !s.is_empty()));
    let name =
        claim_string(&principal.claims, "name").or_else(|| match (&given_name, &family_name) {
            (Some(first), Some(last)) => Some(format!("{first} {last}")),
            (Some(first), _) => Some(first.clone()),
            (_, Some(last)) => Some(last.clone()),
            _ => principal.preferred_username.clone(),
        });
    let profile = SessionProfile {
        id: profile_id,
        name,
        family_name,
        given_name,
        email: principal.email.clone(),
        username: principal.preferred_username.clone(),
        roles: principal.roles.clone(),
        org_id,
        usr_id: Some(uuid::Uuid::parse_str(&principal.sub).unwrap_or(profile_id)),
    };
    let org_name = claim_string(&principal.claims, "org_name").or_else(|| {
        let trimmed = config.mock_org_name.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });
    let organization = org_id.map(|id| SessionOrganization { id, name: org_name });
    Ok(AuthorizationContext {
        realm: "mock".into(),
        user: None,
        profile: Some(profile),
        organization,
        iamroles: principal
            .roles
            .iter()
            .filter(|r| matches!(r.as_str(), "sysadmin" | "operator" | "admin"))
            .cloned()
            .collect(),
        tenant: principal.tenant().or(org_id),
    })
}

fn claim_string(claims: &Value, key: &str) -> Option<String> {
    claims
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AuthProvider;

    fn mock_config() -> AuthConfig {
        let mut config = AuthConfig::default();
        config.auth_provider = AuthProvider::MockAuth;
        config.mock_sub = "b5db55cf-bd20-450e-9555-dd7ba125e9d2".into();
        config.mock_org_id = "00000000-0000-4000-8000-000000000011".into();
        config.mock_org_name = "Dev Seller Org".into();
        config.mock_username = Some("dev-seller".into());
        config
    }

    fn picker_session(config: &AuthConfig) -> Value {
        json!({
            "sub": config.mock_sub,
            "preferred_username": config.mock_username,
            "roles": ["gfs_reader"]
        })
    }

    #[test]
    fn mock_authorization_context_uses_mock_org_from_config() {
        let config = mock_config();
        let claims = picker_session(&config);
        let ctx = mock_authorization_context_from_headers(&config, None, Some(&claims))
            .expect("mock context");
        let profile = ctx.profile.expect("profile");
        assert_eq!(
            profile.org_id,
            Some(uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000011").unwrap())
        );
        let org = ctx.organization.expect("organization");
        assert_eq!(org.id.to_string(), "00000000-0000-4000-8000-000000000011");
        assert_eq!(org.name.as_deref(), Some("Dev Seller Org"));
        assert_eq!(ctx.tenant, Some(org.id));
    }

    #[test]
    fn mock_authorization_context_falls_back_org_id_to_profile() {
        let mut config = AuthConfig::default();
        config.auth_provider = AuthProvider::MockAuth;
        config.mock_sub = "b5db55cf-bd20-450e-9555-dd7ba125e9d2".into();
        let claims = picker_session(&config);
        let ctx = mock_authorization_context_from_headers(&config, None, Some(&claims))
            .expect("mock context");
        let profile = ctx.profile.expect("profile");
        assert_eq!(
            profile.org_id,
            Some(uuid::Uuid::parse_str("b5db55cf-bd20-450e-9555-dd7ba125e9d2").unwrap())
        );
        assert!(ctx.organization.is_some());
        assert_eq!(ctx.tenant, profile.org_id);
        assert!(ctx.organization.unwrap().name.is_none());
    }

    #[test]
    fn mock_authorization_context_prefers_mock_tenant() {
        let mut config = mock_config();
        config.mock_org_id = "00000000-0000-4000-8000-000000000021".into();
        config.mock_tenant = "00000000-0000-4000-8000-000000000011".into();
        let mut claims = picker_session(&config);
        claims
            .as_object_mut()
            .unwrap()
            .insert("_tenant".into(), json!(config.mock_tenant));
        let ctx = mock_authorization_context_from_headers(&config, None, Some(&claims))
            .expect("mock context");
        assert_eq!(
            ctx.organization.expect("organization").id.to_string(),
            "00000000-0000-4000-8000-000000000021"
        );
        assert_eq!(
            ctx.tenant,
            Some(uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000011").unwrap())
        );
    }

    #[test]
    fn mock_authorization_context_requires_session_or_header() {
        let config = mock_config();
        let err = mock_authorization_context_from_headers(&config, None, None).expect_err("anon");
        assert_eq!(err.errcode.as_str(), "AUT-195");
        let claims = picker_session(&config);
        mock_authorization_context_from_headers(&config, None, Some(&claims)).expect("signed in");
    }

    #[test]
    fn sign_in_landing_keeps_configured_spa() {
        assert_eq!(sign_in_landing("/ui/exp.system/"), "/ui/exp.system/");
        assert_eq!(sign_in_landing("  /api/auth/info  "), "/api/auth/info");
        assert_eq!(sign_in_landing(""), "/");
    }

    #[test]
    fn mock_auth_home_logged_in_goes_to_spa() {
        let mut config = mock_config();
        config.base_path = "/api/auth".into();
        config.default_signin_redirect_uri = "/ui/manager/".into();
        let claims = picker_session(&config);
        let authenticated =
            mock_authorization_context_from_headers(&config, None, Some(&claims)).is_ok();
        assert!(authenticated);
        let loc = auth_home_location(&config, None);
        assert_eq!(loc, "/ui/manager/");
    }

    #[test]
    fn mock_auth_home_anon_starts_idp() {
        let mut config = mock_config();
        config.base_path = "/api/auth".into();
        config.default_callback_uri = "https://stratify.localhost/v1/manager/auth/callback".into();
        let authenticated = mock_authorization_context_from_headers(&config, None, None).is_ok();
        assert!(!authenticated);
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-prefix", "/v1/manager".parse().unwrap());
        let fallback = prefix_from_callback_uri(&config.default_callback_uri);
        let auth_base =
            public_api_path(config.base_path.trim_end_matches('/'), &headers, &fallback);
        let callback = validate_redirect_url(
            "",
            &config.default_callback_uri,
            &config.safe_redirect_domains,
            false,
        );
        let (url, _, _, _) =
            mock_authorize_url(&auth_base, config.oauth2_client_id.trim(), &callback);
        assert!(url.starts_with("/v1/manager/auth/mock-auth?"));
        assert!(!url.contains("/sign-in"));
    }

    #[test]
    fn callback_without_code_is_an_error() {
        let err =
            crate::errors::AUT_153.with_data(json!({ "detail": "Missing authorization code" }));
        assert_eq!(err.errcode.as_str(), "AUT-153");
    }
}
