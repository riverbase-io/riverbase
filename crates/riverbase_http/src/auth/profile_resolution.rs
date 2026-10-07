//! Active profile resolution: auth mode detection and request hints.

use axum::http::header::HeaderName;
use axum::http::request::Parts;
use tower_sessions::Session;
use uuid::Uuid;

use super::context::KeycloakTokenPayload;
use crate::config::{AuthConfig, X_PROFILE_HEADER};

/// How the caller authenticated (drives profile resolution order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    /// OAuth session (`id_token` cookie + `session[user]`).
    Session,
    /// Stateless `Authorization: Bearer` only.
    Bearer,
}

/// Input for [`super::AuthProfileProvider::setup_context`].
#[derive(Debug, Clone)]
pub struct SetupContextRequest {
    /// Auth user.
    pub auth_user: KeycloakTokenPayload,
    /// Auth mode.
    pub auth_mode: AuthMode,
    /// Explicit profile id from `X-Profile` or session / JWT (steps 1–2).
    pub profile_hint: Option<Uuid>,
}

static X_PROFILE_HEADER_NAME: std::sync::OnceLock<HeaderName> = std::sync::OnceLock::new();

fn x_profile_header_name() -> &'static HeaderName {
    X_PROFILE_HEADER_NAME.get_or_init(|| {
        HeaderName::from_bytes(X_PROFILE_HEADER.as_bytes()).expect("valid x-profile header name")
    })
}

use crate::base::RiverbaseResult;

/// Parse `X-Profile` header value as a UUID (legacy helper; prefer [`resolve_profile_hint_from_header`]).
#[allow(dead_code)]
pub fn profile_id_from_header(parts: &Parts) -> Option<Uuid> {
    let raw = parts
        .headers
        .get(x_profile_header_name())
        .and_then(|v| v.to_str().ok())?;
    Uuid::parse_str(raw.trim()).ok()
}

/// Resolve profile hint from `X-Profile`, failing closed on malformed values ([SEC-11]).
pub fn resolve_profile_hint_from_header(parts: &Parts) -> RiverbaseResult<Option<Uuid>> {
    let Some(raw) = parts
        .headers
        .get(x_profile_header_name())
        .and_then(|v| v.to_str().ok())
    else {
        return Ok(None);
    };
    Uuid::parse_str(raw.trim())
        .map(Some)
        .map_err(|_| crate::errors::AUT_174.with_data(format!("invalid X-Profile: {raw}")))
}

/// Resolve steps 1–2 profile hint before IDM `setup_context`.
pub async fn resolve_profile_hint(
    config: &AuthConfig,
    parts: &Parts,
    session: Option<&Session>,
    auth_mode: AuthMode,
    auth_user: &KeycloakTokenPayload,
) -> RiverbaseResult<Option<Uuid>> {
    if let Some(id) = resolve_profile_hint_from_header(parts)? {
        return Ok(Some(id));
    }
    match auth_mode {
        AuthMode::Session => {
            if let Some(session) = session {
                if let Ok(Some(raw)) = session
                    .get::<String>(&config.ses_active_profile_field)
                    .await
                {
                    if let Ok(id) = Uuid::parse_str(raw.trim()) {
                        return Ok(Some(id));
                    }
                }
            }
        }
        AuthMode::Bearer => {
            if let Some(id) = auth_user.profile_id {
                return Ok(Some(id));
            }
        }
    }
    Ok(None)
}

/// Persist active profile id in the browser session.
pub async fn persist_active_profile_session(
    config: &AuthConfig,
    session: &Session,
    profile_id: Uuid,
) -> Result<(), tower_sessions::session::Error> {
    session
        .insert(&config.ses_active_profile_field, profile_id.to_string())
        .await
}
