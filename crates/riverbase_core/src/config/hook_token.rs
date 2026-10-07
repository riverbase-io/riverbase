//! Signed command hook tokens (`:hook` HTTP method).

use serde::Deserialize;

use crate::base::RiverbaseResult;

/// Deprecated published default. Fatal at startup unless `default-hook-secret` is allowed.
#[deprecated(note = "do not use as a signing secret; unset is representable as None")]
pub const DEFAULT_HOOK_TOKEN_SECRET: &str = "A7777304-0280-478F-ACF2-58604A026872";

/// Deprecated published default. Fatal at startup unless `default-hook-salt` is allowed.
#[deprecated(note = "do not use as a signing salt; unset is representable as None")]
pub const DEFAULT_HOOK_TOKEN_SALT: &str = "riverbase.fastapi.hook-token";

/// Configuration for signing and verifying command `:hook` tokens.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct HookTokenConfig {
    /// Secret.
    pub secret: Option<String>,
    /// Salt.
    pub salt: Option<String>,
}

impl HookTokenConfig {
    /// Build from env.
    pub fn from_env() -> Self {
        let mut cfg = Self::default();
        if let Ok(secret) = std::env::var("RIVERBASE_HOOK_TOKEN_SECRET") {
            if !secret.trim().is_empty() {
                cfg.secret = Some(secret);
            }
        }
        if let Ok(salt) = std::env::var("RIVERBASE_HOOK_TOKEN_SALT") {
            if !salt.trim().is_empty() {
                cfg.salt = Some(salt);
            }
        }
        cfg
    }

    /// Require secret.
    pub fn require_secret(&self) -> RiverbaseResult<&str> {
        self.secret
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| crate::errors::CFG_143.raise())
    }

    /// Require salt.
    pub fn require_salt(&self) -> RiverbaseResult<&str> {
        self.salt
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| crate::errors::CFG_144.raise())
    }
}
