//! Write identity into the browser session (shared by Keycloak and MockAuth callbacks).

use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::{Cookie as AxumCookie, SameSite};
use axum_extra::extract::CookieJar;
use serde_json::{json, Value};
use tower_sessions::Session;
use tracing::info;

use super::context::KeycloakTokenPayload;
use super::profile_resolution::{persist_active_profile_session, AuthMode, SetupContextRequest};
use super::provider::AuthProfileProvider;
use super::session_helper::{random_token, validate_redirect_url};
use crate::base::RiverbaseError;
use crate::config::AuthConfig;

/// Cycle the session id, persist `ses_user`, upsert profile, redirect to `next`.
pub async fn establish_browser_session(
    session: &Session,
    config: &AuthConfig,
    profile_provider: &dyn AuthProfileProvider,
    mut id_data: Value,
    id_token: &str,
) -> Result<Response, RiverbaseError> {
    let client_token = match session.get::<String>(&config.ses_client_token_field).await {
        Ok(Some(t)) => t,
        _ => {
            let t = random_token();
            session
                .insert(&config.ses_client_token_field, t.clone())
                .await
                .map_err(|e| {
                    crate::errors::AUT_159.with_data(json!({ "detail": e.to_string() }))
                })?;
            t
        }
    };
    let session_id = match session.get::<String>(&config.ses_session_id_field).await {
        Ok(Some(t)) => t,
        _ => {
            let t = random_token();
            session
                .insert(&config.ses_session_id_field, t.clone())
                .await
                .map_err(|e| {
                    crate::errors::AUT_159.with_data(json!({ "detail": e.to_string() }))
                })?;
            t
        }
    };
    if let Some(obj) = id_data.as_object_mut() {
        obj.insert("client_token".into(), json!(client_token));
        obj.insert("session_id".into(), json!(session_id));
    }

    session
        .cycle_id()
        .await
        .map_err(|_e| crate::errors::AUT_155.with_data(json!({})))?;

    session
        .insert(&config.ses_user_field, id_data.clone())
        .await
        .map_err(|e| crate::errors::AUT_159.with_data(json!({ "detail": e.to_string() })))?;

    if let Ok(auth_user) = KeycloakTokenPayload::from_claims(id_data.clone()) {
        super::realm_role::require_configured_realm_role(&auth_user, &config.require_realm_role)?;
        profile_provider.upsert_on_login(&auth_user).await?;
        let setup = SetupContextRequest {
            auth_user,
            auth_mode: AuthMode::Session,
            profile_hint: None,
        };
        if let Ok(ctx) = profile_provider.setup_context(setup).await {
            if let Some(profile) = ctx.profile.as_ref() {
                let _ = persist_active_profile_session(config, session, profile.id).await;
            }
        }
    }

    let next_url = {
        let next: Option<String> = session.get("next").await.ok().flatten();
        validate_redirect_url(
            next.as_deref().unwrap_or(""),
            &config.default_signin_redirect_uri,
            &config.safe_redirect_domains,
            true,
        )
    };

    info!(user_sub = ?id_data.get("sub"), "authorization_success");
    let mut jar = CookieJar::new();
    jar = jar.add(build_id_token_cookie(config, id_token));
    Ok((jar, Redirect::to(&next_url)).into_response())
}

/// Session cookie holding the ID token (httpOnly).
pub fn build_id_token_cookie(config: &AuthConfig, value: &str) -> AxumCookie<'static> {
    let same_site = parse_same_site(&config.cookie_same_site);
    AxumCookie::build((config.ses_id_token_field.clone(), value.to_string()))
        .http_only(true)
        .secure(config.cookie_https_only)
        .same_site(same_site)
        .path("/")
        .into()
}

/// Removal cookie for the ID token.
pub fn id_token_removal_cookie(config: &AuthConfig) -> AxumCookie<'static> {
    let same_site = parse_same_site(&config.cookie_same_site);
    AxumCookie::build((config.ses_id_token_field.clone(), ""))
        .http_only(true)
        .secure(config.cookie_https_only)
        .same_site(same_site)
        .path("/")
        .removal()
        .into()
}

fn parse_same_site(policy: &str) -> SameSite {
    match policy.to_ascii_lowercase().as_str() {
        "lax" => SameSite::Lax,
        "none" => SameSite::None,
        _ => SameSite::Strict,
    }
}
