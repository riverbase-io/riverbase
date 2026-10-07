use std::sync::Arc;

use aide::axum::{routing::get_with, routing::post_with, ApiRouter};
use aide::transform::TransformOperation;
use aide::NoApi;
use axum::{
    extract::{Path, Query},
    http::HeaderMap,
    middleware,
    response::{IntoResponse, Redirect, Response},
    Extension, Json,
};
use axum_extra::extract::CookieJar;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use tower_sessions::Session;
use tracing::info;

use super::callback_error::{callback_failure_response, negotiate_auth_error_html};
use super::context::AuthorizationContext;
use super::csrf::{generate_csrf_token, validate_csrf_token};
use super::oauth::{merge_token_claims, KeycloakOAuth};
use super::profile_resolution::{
    persist_active_profile_session, resolve_profile_hint, resolve_profile_hint_from_header,
    SetupContextRequest,
};
use super::provider::{AuthProfileProvider, DefaultAuthProfileProvider};
use super::session_establish::{establish_browser_session, id_token_removal_cookie};
use super::session_helper::{auth_home_location, validate_redirect_url};
use super::validator::JwtValidator;
use crate::base::{RiverbaseError, RiverbaseResult};
use crate::config::AuthConfig;
use crate::openapi_meta::OpenApiMeta;
use crate::web::openapi::{apply_riverbase_operation, RiverbaseOperationKind, RiverbaseOperationMeta};

/// Shared state for `/auth` routes.
#[derive(Clone)]
pub struct AuthState {
    /// Config.
    pub config: AuthConfig,
    /// Validator.
    pub validator: Arc<JwtValidator>,
    /// Provider.
    pub provider: DefaultAuthProfileProvider,
    /// Profile provider.
    pub profile_provider: Arc<dyn AuthProfileProvider>,
    /// Oauth.
    pub oauth: KeycloakOAuth,
}

impl AuthState {
    /// Build.
    pub async fn build(config: AuthConfig) -> RiverbaseResult<Self> {
        Self::build_with_profile_provider(config, None).await
    }

    /// Build with profile provider.
    pub async fn build_with_profile_provider(
        config: AuthConfig,
        profile_provider: Option<Arc<dyn AuthProfileProvider>>,
    ) -> RiverbaseResult<Self> {
        let oidc = crate::auth::to_oidc_config(&config);
        let validator = Arc::new(JwtValidator::new(oidc)?);
        validator.warmup().await?;
        let oauth = KeycloakOAuth::new(config.clone())?;
        let provider = DefaultAuthProfileProvider::new(config.clone(), validator.clone());
        let profile_provider = profile_provider.unwrap_or_else(|| Arc::new(provider.clone()));
        Ok(Self {
            config,
            validator,
            provider,
            profile_provider,
            oauth,
        })
    }
}

pub(crate) fn auth_op(
    operation_id: impl Into<String>,
    summary: impl Into<String>,
    description: impl Into<String>,
    api_segment: impl Into<String>,
    kind: RiverbaseOperationKind,
) -> impl FnOnce(TransformOperation) -> TransformOperation {
    let operation_id = operation_id.into();
    let summary = summary.into();
    let description = description.into();
    let api_segment = api_segment.into();
    move |op| {
        let meta = RiverbaseOperationMeta {
            namespace: "auth".into(),
            segment: api_segment,
            kind,
            scoped: false,
            openapi: OpenApiMeta::default().with_tag("riverbase:auth"),
            command_key: None,
            resources: Vec::new(),
        };
        let http_method = match kind {
            RiverbaseOperationKind::GenericPost => "post",
            _ => "get",
        };
        apply_riverbase_operation(op, &meta, http_method)
            .id(&operation_id)
            .summary(&summary)
            .description(&description)
    }
}

/// Build the `/auth` API router (Python `configure_authentication` routes).
pub fn auth_router(state: Arc<AuthState>) -> ApiRouter {
    let base = state.config.base_path.trim_end_matches('/').to_string();
    let home = format!("{base}/");
    let callback = format!("{base}/callback");
    let verify = format!("{base}/verify");
    let info = format!("{base}/info");
    let profiles_path = format!("{base}/profiles");
    let switch_profile_path = format!("{base}/switch-profile/{{profile_id}}");
    let csrf = format!("{base}/csrf-token");
    let path_home = format!("{base}/home");
    let path_sign_in = format!("{base}/sign-in");
    let path_sign_up = format!("{base}/sign-up");
    let path_sign_out = format!("{base}/sign-out");

    let public = ApiRouter::new()
        .api_route(
            home.as_str(),
            get_with(auth_home, |op| {
                auth_op(
                    "auth_home",
                    "Auth home",
                    "Entry point for browser OAuth2 login with Keycloak.",
                    "",
                    RiverbaseOperationKind::GenericGet,
                )(op)
                .response_with::<200, Json<Value>, _>(|res| res.description("Auth entry message"))
            }),
        )
        .api_route(
            callback.as_str(),
            get_with(
                oauth_callback,
                auth_op(
                    "auth_callback",
                    "OAuth callback",
                    "Exchange authorization code for tokens and establish session.",
                    "callback",
                    RiverbaseOperationKind::GenericGet,
                ),
            ),
        )
        .api_route(
            csrf.as_str(),
            get_with(get_csrf_token, |op| {
                auth_op(
                    "auth_csrf_token",
                    "CSRF token",
                    "Return or create a CSRF token for the current session.",
                    "csrf-token",
                    RiverbaseOperationKind::GenericGet,
                )(op)
                .response_with::<200, Json<Value>, _>(|res| res.description("CSRF token payload"))
            }),
        )
        .api_route(
            path_home.as_str(),
            get_with(
                auth_session_home,
                auth_op(
                    "auth_session_home",
                    "Auth session home",
                    "Redirect to the SPA when the browser session is valid, otherwise start the IdP.",
                    "home",
                    RiverbaseOperationKind::GenericGet,
                ),
            ),
        )
        .api_route(
            path_sign_in.as_str(),
            get_with(
                sign_in,
                auth_op(
                    "auth_sign_in",
                    "Sign in",
                    "Start OAuth2 authorization with Keycloak.",
                    "sign-in",
                    RiverbaseOperationKind::GenericGet,
                ),
            ),
        )
        .api_route(
            path_sign_up.as_str(),
            get_with(
                sign_up,
                auth_op(
                    "auth_sign_up",
                    "Sign up",
                    "Redirect to Keycloak registration.",
                    "sign-up",
                    RiverbaseOperationKind::GenericGet,
                ),
            ),
        )
        .api_route(
            path_sign_out.as_str(),
            get_with(
                sign_out,
                auth_op(
                    "auth_sign_out",
                    "Sign out",
                    "End session and redirect to logout URL.",
                    "sign-out",
                    RiverbaseOperationKind::GenericGet,
                ),
            ),
        );

    let protected = ApiRouter::new()
        .api_route(
            verify.as_str(),
            get_with(verify_auth, |op| {
                auth_op(
                    "auth_verify",
                    "Verify session",
                    "Confirm the caller is authenticated and return context.",
                    "verify",
                    RiverbaseOperationKind::GenericGet,
                )(op)
                .response_with::<200, Json<Value>, _>(|res| {
                    res.description("Authenticated session details")
                })
            }),
        )
        .api_route(
            info.as_str(),
            get_with(auth_info, |op| {
                auth_op(
                    "auth_info",
                    "Auth info",
                    "Return resolved authorization context for the current session.",
                    "info",
                    RiverbaseOperationKind::GenericGet,
                )(op)
                .response_with::<200, Json<AuthorizationContext>, _>(|res| {
                    res.description("Authorization context")
                })
            }),
        )
        .api_route(
            profiles_path.as_str(),
            get_with(auth_profiles, |op| {
                auth_op(
                    "auth_profiles",
                    "List profiles",
                    "List IDM profiles for the authenticated user.",
                    "profiles",
                    RiverbaseOperationKind::GenericGet,
                )(op)
                .response_with::<200, Json<Value>, _>(|res| res.description("Profile list"))
            }),
        )
        .api_route(
            switch_profile_path.as_str(),
            post_with(auth_switch_profile, |op| {
                auth_op(
                    "auth_switch_profile",
                    "Switch profile",
                    "Set the active profile for the current session and database.",
                    "switch-profile",
                    RiverbaseOperationKind::GenericPost,
                )(op)
                .response_with::<200, Json<AuthorizationContext>, _>(|res| {
                    res.description("Updated authorization context")
                })
            }),
        );

    let html_config = state.config.clone();
    public
        .merge(protected)
        .layer(Extension(state))
        .layer(middleware::from_fn_with_state(html_config, negotiate_auth_error_html))
}

async fn auth_home(Extension(state): Extension<Arc<AuthState>>) -> Json<Value> {
    let sign_in = format!("{}/sign-in", state.config.base_path.trim_end_matches('/'));
    Json(json!({
        "message": format!("Go to {sign_in} to start OAuth2 login with Keycloak")
    }))
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    #[allow(dead_code)]
    session_state: Option<String>,
    error: Option<String>,
}

async fn oauth_callback(
    Extension(state): Extension<Arc<AuthState>>,
    NoApi(session): NoApi<Session>,
    NoApi(jar): NoApi<CookieJar>,
    headers: HeaderMap,
    Query(params): Query<CallbackQuery>,
) -> Response {
    match oauth_callback_result(&state, &session, params).await {
        Ok(response) => response,
        Err(err) => callback_failure_response(&session, &state.config, jar, &headers, err).await,
    }
}

async fn oauth_callback_result(
    state: &AuthState,
    session: &Session,
    params: CallbackQuery,
) -> Result<Response, RiverbaseError> {
    if params.error.is_some() {
        return Err(crate::errors::AUT_152.with_data(json!({})));
    }
    let code = params
        .code
        .ok_or_else(|| crate::errors::AUT_153.with_data(json!({})))?;

    let (pkce_key, state_key) = KeycloakOAuth::session_keys();
    let pkce_verifier: Option<String> = session.get(pkce_key).await.ok().flatten();
    let expected_state: Option<String> = session.get(state_key).await.ok().flatten();

    let pkce_verifier = pkce_verifier.ok_or_else(|| crate::errors::AUT_149.with_data(json!({})))?;

    let redirect_uri: String = session
        .get(KeycloakOAuth::redirect_uri_session_key())
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| state.config.default_callback_uri.clone());
    let tokens = state
        .oauth
        .exchange_code(
            &code,
            &redirect_uri,
            &pkce_verifier,
            expected_state.as_deref(),
            params.state.as_deref(),
        )
        .await?;

    let id_token = tokens
        .id_token
        .ok_or_else(|| crate::errors::AUT_154.with_data(json!({})))?;
    let ac_token = tokens.access_token.unwrap_or_default();

    let mut id_data = state
        .validator
        .decode_id_token_claims(&id_token)
        .await?
        .claims;
    if !ac_token.is_empty() {
        // Gitea access tokens may be opaque or omit Keycloak-style `azp`.
        if let Ok(ac_data) = state.validator.decode_access_token_claims(&ac_token).await {
            merge_token_claims(&mut id_data, &ac_data.claims);
        }
    }

    establish_browser_session(
        session,
        &state.config,
        state.profile_provider.as_ref(),
        id_data,
        &id_token,
    )
    .await
}

async fn verify_auth(
    Extension(state): Extension<Arc<AuthState>>,
    NoApi(session): NoApi<Session>,
    NoApi(jar): NoApi<CookieJar>,
    headers: HeaderMap,
) -> Result<Json<Value>, RiverbaseError> {
    let ctx = require_auth(&state, &session, &jar, &headers).await?;
    if ctx.user.is_none() {
        return Err(crate::errors::AUT_156.with_data(json!({})));
    }
    Ok(Json(json!({
        "status": "OK",
        "message": "User logged in.",
        "context": ctx,
        "headers": { "cookie": "<redacted>" }
    })))
}

async fn auth_info(
    Extension(state): Extension<Arc<AuthState>>,
    NoApi(session): NoApi<Session>,
    NoApi(jar): NoApi<CookieJar>,
    headers: HeaderMap,
) -> Result<Json<AuthorizationContext>, RiverbaseError> {
    let ctx = require_auth(&state, &session, &jar, &headers).await?;
    Ok(Json(ctx))
}

async fn auth_profiles(
    Extension(state): Extension<Arc<AuthState>>,
    NoApi(session): NoApi<Session>,
    NoApi(jar): NoApi<CookieJar>,
    headers: HeaderMap,
) -> Result<Json<Value>, RiverbaseError> {
    let ctx = require_auth(&state, &session, &jar, &headers).await?;
    let profiles = state.profile_provider.list_profiles(&ctx, None).await?;
    Ok(Json(json!({ "profiles": profiles })))
}

async fn auth_switch_profile(
    Extension(state): Extension<Arc<AuthState>>,
    Path(profile_id): Path<uuid::Uuid>,
    NoApi(session): NoApi<Session>,
    NoApi(jar): NoApi<CookieJar>,
    headers: HeaderMap,
) -> Result<Json<AuthorizationContext>, RiverbaseError> {
    let parts = headers_to_parts(&headers);
    let Some((token, auth_mode)) = state
        .provider
        .get_auth_token_with_mode(&parts, Some(&session), &jar)
        .await?
    else {
        return Err(crate::errors::AUT_158.with_data(json!({})));
    };
    let auth_user = state.provider.authorize_claims(token)?;
    let profile_hint = match resolve_profile_hint_from_header(&parts)? {
        Some(id) => Some(id),
        None => {
            resolve_profile_hint(&state.config, &parts, Some(&session), auth_mode, &auth_user)
                .await?
        }
    };
    let request = SetupContextRequest {
        auth_user,
        auth_mode,
        profile_hint,
    };
    let ctx = state
        .profile_provider
        .switch_profile(request, profile_id)
        .await?;
    persist_active_profile_session(&state.config, &session, profile_id)
        .await
        .map_err(|e| crate::errors::AUT_159.with_data(json!({ "detail": e.to_string() })))?;
    Ok(Json(ctx))
}

async fn require_auth(
    state: &AuthState,
    session: &Session,
    jar: &CookieJar,
    headers: &HeaderMap,
) -> Result<AuthorizationContext, RiverbaseError> {
    let parts = headers_to_parts(headers);
    let ctx = state
        .provider
        .get_auth_context(state.profile_provider.as_ref(), &parts, Some(session), jar)
        .await?;
    ctx.ok_or_else(|| crate::errors::AUT_158.with_data(json!({})))
}

fn headers_to_parts(headers: &HeaderMap) -> axum::http::request::Parts {
    let mut req = axum::http::Request::builder().body(()).unwrap();
    *req.headers_mut() = headers.clone();
    req.into_parts().0
}

async fn get_csrf_token(NoApi(session): NoApi<Session>) -> Result<Json<Value>, RiverbaseError> {
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

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SignInQuery {
    pub next: Option<String>,
    pub callback: Option<String>,
}

async fn auth_session_home(
    Extension(state): Extension<Arc<AuthState>>,
    NoApi(session): NoApi<Session>,
    NoApi(jar): NoApi<CookieJar>,
    headers: HeaderMap,
    Query(params): Query<SignInQuery>,
) -> Result<Redirect, RiverbaseError> {
    let authenticated = require_auth(&state, &session, &jar, &headers).await.is_ok();
    if authenticated {
        return Ok(Redirect::to(&auth_home_location(
            &state.config,
            params.next.as_deref(),
        )));
    }
    start_oauth_sign_in(&state, &session, &params).await
}

/// Store PKCE/state and redirect to the IdP. Shared by `/auth/home` (anonymous) and `/auth/sign-in`.
async fn start_oauth_sign_in(
    state: &AuthState,
    session: &Session,
    params: &SignInQuery,
) -> Result<Redirect, RiverbaseError> {
    let csrf_token = generate_csrf_token();
    session
        .insert("csrf_token", csrf_token)
        .await
        .map_err(|e| crate::errors::AUT_159.with_data(json!({ "detail": e.to_string() })))?;
    if let Some(next) = &params.next {
        session
            .insert("next", next.clone())
            .await
            .map_err(|e| crate::errors::AUT_159.with_data(json!({ "detail": e.to_string() })))?;
    }

    let callback_uri = validate_redirect_url(
        params.callback.as_deref().unwrap_or(""),
        &state.config.default_callback_uri,
        &state.config.safe_redirect_domains,
        false,
    );

    let (url, pkce_verifier, oauth_state) = state.oauth.authorize_url(&callback_uri)?;
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

async fn sign_in(
    Extension(state): Extension<Arc<AuthState>>,
    NoApi(session): NoApi<Session>,
    Query(params): Query<SignInQuery>,
) -> Result<Redirect, RiverbaseError> {
    start_oauth_sign_in(&state, &session, &params).await
}

async fn sign_up(Extension(state): Extension<Arc<AuthState>>) -> Redirect {
    Redirect::to(state.oauth.signup_url().as_str())
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SignOutQuery {
    pub csrf_token: Option<String>,
    pub redirect_uri: Option<String>,
    /// SPA alias for [`Self::redirect_uri`] (`?redirect=/`).
    pub redirect: Option<String>,
}

pub(crate) fn sign_out_redirect_target(
    params: &SignOutQuery,
    headers: &HeaderMap,
    default: &str,
    whitelist: &[String],
    cache_invalidate: bool,
) -> String {
    let raw = params
        .redirect_uri
        .as_deref()
        .or(params.redirect.as_deref())
        .or_else(|| headers.get("X-Redirect-Uri").and_then(|v| v.to_str().ok()))
        .unwrap_or("");
    validate_redirect_url(raw, default, whitelist, cache_invalidate)
}

async fn sign_out(
    Extension(state): Extension<Arc<AuthState>>,
    NoApi(session): NoApi<Session>,
    NoApi(jar): NoApi<CookieJar>,
    Query(params): Query<SignOutQuery>,
    headers: axum::http::HeaderMap,
) -> Result<Response, RiverbaseError> {
    let cfg = &state.config;
    if cfg.validate_csrf_token {
        let token = params
            .csrf_token
            .clone()
            .or_else(|| {
                headers
                    .get("X-CSRF-Token")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string())
            })
            .unwrap_or_default();
        let session_csrf: Option<String> = session.get("csrf_token").await.ok().flatten();
        if !validate_csrf_token(session_csrf.as_deref(), &token) {
            return Err(crate::errors::AUT_157.with_data(json!({})));
        }
    }

    let redirect_uri = sign_out_redirect_target(
        &params,
        &headers,
        &cfg.default_logout_redirect_uri,
        &cfg.safe_redirect_domains,
        true,
    );

    let id_data: Option<Value> = session.get(&cfg.ses_user_field).await.ok().flatten();
    let id_token = jar
        .get(&cfg.ses_id_token_field)
        .map(|c| c.value().to_string());

    let logout_url = match (id_token.as_deref(), id_data.as_ref()) {
        (Some(token), Some(_)) => state.oauth.logout_redirect(token, &redirect_uri),
        _ => redirect_uri.clone(),
    };

    if id_data.is_some() {
        info!("user_logout");
    }

    session
        .flush()
        .await
        .map_err(|e| crate::errors::AUT_159.with_data(json!({ "detail": e.to_string() })))?;

    let mut out_jar = jar;
    out_jar = out_jar.remove(id_token_removal_cookie(cfg));
    Ok((out_jar, Redirect::to(&logout_url)).into_response())
}
