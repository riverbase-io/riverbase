//! Dangerous configuration checks and startup report ([CFG-01], [SEC-09], [OPS-06]).

use std::collections::BTreeSet;

use tracing::{info, warn};

use crate::base::RiverbaseResult;
use crate::config::{AuthProvider, RiverbaseConfig};
#[allow(deprecated)]
use crate::config::{
    DEFAULT_HOOK_TOKEN_SALT, DEFAULT_HOOK_TOKEN_SECRET, DEFAULT_LINK_TOKEN_SALT,
    DEFAULT_LINK_TOKEN_SECRET,
};

#[cfg(feature = "auth")]
use super::auth_apply::{auth_disabled, flrs_auth_force_disabled};

/// Published session-signing default. Fatal unless `default-session-secret` is allowed.
pub const PUBLISHED_SESSION_SECRET: &str = "super-secret-session-key-IUUUCBhv4NRDVB4ONpe8lcNJJY";

/// Per-check names accepted by `RIVERBASE_INSECURE_ALLOW`.
pub const ALLOW_DEFAULT_SESSION_SECRET: &str = "default-session-secret";
/// Allow Default Hook Secret constant.
pub const ALLOW_DEFAULT_HOOK_SECRET: &str = "default-hook-secret";
/// Allow Default Hook Salt constant.
pub const ALLOW_DEFAULT_HOOK_SALT: &str = "default-hook-salt";
/// Allow Default Link Secret constant.
pub const ALLOW_DEFAULT_LINK_SECRET: &str = "default-link-secret";
/// Allow Default Link Salt constant.
pub const ALLOW_DEFAULT_LINK_SALT: &str = "default-link-salt";
/// Allow Auth Disabled With Casbin constant.
pub const ALLOW_AUTH_DISABLED_WITH_CASBIN: &str = "auth-disabled-with-casbin";
/// Allow Omit Openapi Without Auth constant.
pub const ALLOW_OMIT_OPENAPI_WITHOUT_AUTH: &str = "omit-openapi-without-auth";
/// Allow Empty Keycloak Issuer constant.
pub const ALLOW_EMPTY_KEYCLOAK_ISSUER: &str = "empty-keycloak-issuer";
/// Allow Insecure Cookies constant.
pub const ALLOW_INSECURE_COOKIES: &str = "insecure-cookies";
/// Allow Empty Azp constant.
pub const ALLOW_EMPTY_AZP: &str = "empty-azp-allowlist";

/// Verbosity flag only. Does not skip security checks ([CFG-01]).
pub fn flrs_development_mode() -> bool {
    std::env::var("RIVERBASE_DEBUG")
        .ok()
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

/// Parse `RIVERBASE_INSECURE_ALLOW` (comma-separated check names).
pub fn insecure_allowances_from_env() -> BTreeSet<String> {
    parse_insecure_allowances(&std::env::var("RIVERBASE_INSECURE_ALLOW").unwrap_or_default())
}

/// Parse insecure allowances.
pub fn parse_insecure_allowances(raw: &str) -> BTreeSet<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| name.to_ascii_lowercase())
        .collect()
}

fn allowed(allowances: &BTreeSet<String>, check: &str) -> bool {
    allowances.contains(check)
}

fn allow_or_err(
    allowances: &BTreeSet<String>,
    check: &str,
    err: crate::base::RiverbaseError,
) -> RiverbaseResult<()> {
    if allowed(allowances, check) {
        warn!(check, "RIVERBASE_INSECURE_ALLOW exemption active");
        Ok(())
    } else {
        Err(err)
    }
}

fn secret_is_missing_or_published(value: Option<&str>, published: &str) -> bool {
    match value.map(str::trim) {
        None | Some("") => true,
        Some(secret) => secret == published,
    }
}

/// Every deployable process must declare at least one deployment zone.
pub fn validate_api_zone(config: &RiverbaseConfig) -> RiverbaseResult<()> {
    if config.api_zone.is_empty() {
        return Err(crate::errors::CFG_145.raise());
    }
    Ok(())
}

/// Refuse to start on dangerous production configuration unless a named allowance is set.
pub fn validate_startup_config(config: &RiverbaseConfig) -> RiverbaseResult<()> {
    validate_startup_config_with(config, &insecure_allowances_from_env())
}

#[allow(deprecated)]
/// Validate startup config with.
pub fn validate_startup_config_with(
    config: &RiverbaseConfig,
    allowances: &BTreeSet<String>,
) -> RiverbaseResult<()> {
    validate_api_zone(config)?;
    config.bus.validate()?;

    #[cfg(feature = "auth")]
    {
        if config.casbin.enabled && (flrs_auth_force_disabled() || auth_disabled(&config.auth)) {
            allow_or_err(
                allowances,
                ALLOW_AUTH_DISABLED_WITH_CASBIN,
                crate::errors::CFG_140.raise(),
            )?;
        }

        if config.casbin.omit_inaccessible_openapi && auth_disabled(&config.auth) {
            allow_or_err(
                allowances,
                ALLOW_OMIT_OPENAPI_WITHOUT_AUTH,
                crate::errors::CFG_132.raise(),
            )?;
        }

        if secret_is_missing_or_published(
            config.auth.application_secret_key.as_deref(),
            PUBLISHED_SESSION_SECRET,
        ) {
            allow_or_err(
                allowances,
                ALLOW_DEFAULT_SESSION_SECRET,
                crate::errors::CFG_141.raise(),
            )?;
        }

        if config.auth.auth_provider == AuthProvider::Keycloak
            && config.auth.effective_issuer().trim().is_empty()
        {
            allow_or_err(
                allowances,
                ALLOW_EMPTY_KEYCLOAK_ISSUER,
                crate::errors::CFG_142.raise(),
            )?;
        }

        if config.auth.auth_provider == AuthProvider::Keycloak
            && config.auth.accepted_azp.is_empty()
        {
            allow_or_err(allowances, ALLOW_EMPTY_AZP, crate::errors::CFG_148.raise())?;
        }

        if !config.auth.cookie_https_only || !config.auth.validate_csrf_token {
            allow_or_err(
                allowances,
                ALLOW_INSECURE_COOKIES,
                crate::errors::CFG_149.raise(),
            )?;
        }
    }

    if secret_is_missing_or_published(
        config.hook_token.secret.as_deref(),
        DEFAULT_HOOK_TOKEN_SECRET,
    ) {
        allow_or_err(
            allowances,
            ALLOW_DEFAULT_HOOK_SECRET,
            crate::errors::CFG_183.raise(),
        )?;
    }
    if secret_is_missing_or_published(config.hook_token.salt.as_deref(), DEFAULT_HOOK_TOKEN_SALT) {
        allow_or_err(
            allowances,
            ALLOW_DEFAULT_HOOK_SALT,
            crate::errors::CFG_184.raise(),
        )?;
    }
    if secret_is_missing_or_published(
        config.link_token.secret.as_deref(),
        DEFAULT_LINK_TOKEN_SECRET,
    ) {
        allow_or_err(
            allowances,
            ALLOW_DEFAULT_LINK_SECRET,
            crate::errors::CFG_181.raise(),
        )?;
    }
    if secret_is_missing_or_published(config.link_token.salt.as_deref(), DEFAULT_LINK_TOKEN_SALT) {
        allow_or_err(
            allowances,
            ALLOW_DEFAULT_LINK_SALT,
            crate::errors::CFG_182.raise(),
        )?;
    }

    Ok(())
}

/// Emit a single structured startup summary (never logs secret values).
#[allow(deprecated)]
pub fn log_startup_report(
    config: &RiverbaseConfig,
    mounted_domains: usize,
    policy_pack: Option<&str>,
    #[cfg(feature = "auth")] route_auth: &super::route_auth::RouteAuthState,
) {
    let allowances = insecure_allowances_from_env();
    for check in &allowances {
        warn!(check, "RIVERBASE_INSECURE_ALLOW exemption active");
    }

    let auth_mode = match config.auth.auth_provider {
        AuthProvider::None => "disabled",
        AuthProvider::MockAuth => "mock",
        AuthProvider::Keycloak => "keycloak",
    };

    let casbin = if config.casbin.enabled {
        "enabled"
    } else {
        "disabled"
    };

    let default_session_secret = secret_is_missing_or_published(
        config.auth.application_secret_key.as_deref(),
        PUBLISHED_SESSION_SECRET,
    );
    let default_hook_secret = secret_is_missing_or_published(
        config.hook_token.secret.as_deref(),
        DEFAULT_HOOK_TOKEN_SECRET,
    );

    #[cfg(feature = "auth")]
    let public_exemptions = route_auth.public_exemptions();

    #[cfg(feature = "auth")]
    info!(
        auth_mode,
        casbin,
        policy_pack,
        mounted_domains,
        public_exemptions = ?public_exemptions,
        insecure_allowances = ?allowances,
        dbpool_max_size = config.dbpool_max_size,
        dbpool_acquire_timeout_secs = config.dbpool_acquire_timeout_secs,
        dbpool_max_lifetime_secs = config.dbpool_max_lifetime_secs,
        database_statement_timeout_ms = config.database_statement_timeout_ms,
        api_base = %config.api_base,
        api_zone = ?config.api_zone,
        tenant = %config.tenant,
        tenant_id = ?config.tenant_id,
        default_session_secret,
        default_hook_secret,
        debug_verbosity = flrs_development_mode(),
        "riverbase startup configuration"
    );

    #[cfg(not(feature = "auth"))]
    info!(
        auth_mode,
        casbin,
        policy_pack,
        mounted_domains,
        insecure_allowances = ?allowances,
        dbpool_max_size = config.dbpool_max_size,
        dbpool_acquire_timeout_secs = config.dbpool_acquire_timeout_secs,
        dbpool_max_lifetime_secs = config.dbpool_max_lifetime_secs,
        database_statement_timeout_ms = config.database_statement_timeout_ms,
        api_base = %config.api_base,
        api_zone = ?config.api_zone,
        tenant = %config.tenant,
        tenant_id = ?config.tenant_id,
        default_session_secret,
        default_hook_secret,
        debug_verbosity = flrs_development_mode(),
        "riverbase startup configuration"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RiverbaseConfig;

    fn zone_config() -> RiverbaseConfig {
        let mut config = RiverbaseConfig::default();
        config.api_zone = vec!["seller".into()];
        config
    }

    #[test]
    fn validate_api_zone_rejects_empty() {
        let config = RiverbaseConfig::default();
        let err = validate_api_zone(&config).expect_err("empty api_zone");
        assert_eq!(err.errcode.as_str(), "CFG-145");
        assert!(err.errdata.is_null());
        assert!(err
            .errhint
            .as_ref()
            .is_some_and(|h| h.contains("RIVERBASE_API_ZONE")));
    }

    #[test]
    #[cfg(feature = "auth")]
    fn validate_startup_config_default_session_secret_uses_errhint() {
        let config = zone_config();
        let err = validate_startup_config_with(&config, &BTreeSet::new())
            .expect_err("default session secret");
        assert_eq!(err.errcode.as_str(), "CFG-141");
        assert!(err.errdata.is_null());
        assert_eq!(
            err.errhint.as_deref(),
            Some(
                "Set [riverbase.auth] application_secret_key or RIVERBASE_INSECURE_ALLOW=default-session-secret."
            )
        );
    }

    #[test]
    #[cfg(feature = "auth")]
    fn riverbase_debug_does_not_bypass_default_secret() {
        let config = zone_config();
        let err = validate_startup_config_with(&config, &BTreeSet::new())
            .expect_err("debug is not an allowance");
        assert_eq!(err.errcode.as_str(), "CFG-141");
    }

    #[test]
    #[cfg(feature = "auth")]
    fn default_session_secret_allowance_enables_only_that_check() {
        let config = zone_config();
        let only_session = BTreeSet::from([ALLOW_DEFAULT_SESSION_SECRET.to_string()]);
        let err = validate_startup_config_with(&config, &only_session)
            .expect_err("other defaults still fail");
        assert_eq!(err.errcode.as_str(), "CFG-143");
    }

    #[test]
    fn validate_api_zone_accepts_nonempty() {
        let config = zone_config();
        validate_api_zone(&config).expect("non-empty api_zone");
    }

    #[test]
    fn parse_allowances_is_comma_separated_and_case_insensitive() {
        let parsed = parse_insecure_allowances("default-session-secret, DEFAULT-HOOK-SECRET");
        assert!(parsed.contains(ALLOW_DEFAULT_SESSION_SECRET));
        assert!(parsed.contains(ALLOW_DEFAULT_HOOK_SECRET));
        assert_eq!(parsed.len(), 2);
    }
}
