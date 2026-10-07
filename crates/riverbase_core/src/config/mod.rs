//! Typed configuration with layered TOML file and environment loading.

mod auth;
mod bus;
mod casbin;
mod hook_token;
mod link_token;
mod media;
mod pool;

use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::base::{tenant_id_from_config, RiverbaseResult};
use crate::logstore::parse_bool_env;
use serde::Deserialize;

pub use crate::logstore::AuditLogConfig;
pub use auth::{AuthConfig, AuthProvider, MockUser, X_PROFILE_HEADER};
pub use bus::{kind_from_url, BusConfig, BusKind};
pub use casbin::CasbinConfig;
#[allow(deprecated)]
pub use hook_token::{HookTokenConfig, DEFAULT_HOOK_TOKEN_SALT, DEFAULT_HOOK_TOKEN_SECRET};
#[allow(deprecated)]
pub use link_token::{LinkTokenConfig, DEFAULT_LINK_TOKEN_SALT, DEFAULT_LINK_TOKEN_SECRET};

/// Seam for resolving secrets from the environment or a future store ([CFG-02], D6).
pub trait SecretProvider: Send + Sync {
    /// Get.
    fn get(&self, name: &str) -> Option<String>;
}

/// Default provider: non-empty environment variables.
#[derive(Debug, Default, Clone, Copy)]
pub struct EnvSecretProvider;

impl SecretProvider for EnvSecretProvider {
    fn get(&self, name: &str) -> Option<String> {
        std::env::var(name)
            .ok()
            .filter(|value| !value.trim().is_empty())
    }
}
pub use media::MediaStorageConfig;
pub use pool::DatabasePoolConfig;

/// Environment variable prefix for Riverbase config overlays (`RIVERBASE_*`).
pub const ENV_PREFIX: &str = "RIVERBASE_";

/// Root section name in TOML files: `[riverbase]`.
pub const CONFIG_SECTION: &str = "riverbase";

/// Environment variable pointing at a TOML config file path.
pub const ENV_CONFIG_PATH: &str = "RIVERBASE_CONFIG";

/// Tracing output format for [`init_logging`](crate::applog::init_logging).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// Single-line human-readable output (default for local dev).
    #[default]
    #[serde(alias = "human")]
    Compact,
    /// Multi-line human-readable output with expanded fields.
    Pretty,
    /// Structured JSON lines (for production log shippers).
    Json,
}

impl LogFormat {
    /// Parse a format hint (`compact`, `human`, `pretty`, `json`). Unknown values → `Compact`.
    pub fn parse_hint(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "json" => Self::Json,
            "pretty" => Self::Pretty,
            "compact" | "human" => Self::Compact,
            _ => Self::Compact,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
/// Riverbase config structure.
pub struct RiverbaseConfig {
    /// Db url.
    pub db_url: String,
    /// Bind addr.
    pub bind_addr: String,
    /// Log level.
    pub log_level: String,
    /// Tracing output format (`compact`, `pretty`, or `json`).
    #[serde(default)]
    pub log_format: LogFormat,
    /// Log each HTTP request (method, URI, status, latency) to the console.
    #[serde(default)]
    pub log_http_requests: bool,
    /// Service mode.
    pub service_mode: String,
    /// Mount prefix for OpenAPI, auth, domain command/query, and RxDB routes (default `/api`).
    #[serde(default = "default_api_base")]
    pub api_base: String,
    /// Audit log.
    pub audit_log: AuditLogConfig,
    /// Auth.
    pub auth: AuthConfig,
    /// Bus.
    pub bus: BusConfig,
    /// Casbin.
    pub casbin: CasbinConfig,
    /// Link token.
    pub link_token: LinkTokenConfig,
    /// Hook token.
    pub hook_token: HookTokenConfig,
    /// Media.
    pub media: MediaStorageConfig,
    /// Raw `[riverbase] tenant` / `RIVERBASE_TENANT` value (UUID literal or name).
    /// Auth `realm` stays on the principal / OIDC token (`profile.realm`, Keycloak).
    #[serde(default, alias = "realm")]
    pub tenant: String,
    /// Data-isolation UUID resolved from [`Self::tenant`] (parse UUID, else SHA-1 hash).
    #[serde(skip)]
    pub tenant_id: Option<Uuid>,
    /// Named tenant access policy (`profile-organization`, `system`, …).
    #[serde(default = "default_tenant_access_policy")]
    pub tenant_access_policy: String,
    /// Named tenant stamp policy (`profile-organization`, `profile`, `user`, `domain-tenant`).
    /// The policy chooses the UUID written when a row is created. Updates keep the stored `_tenant`.
    #[serde(default = "default_tenant_stamp_policy")]
    pub tenant_stamp_policy: String,
    /// Max Postgres connections (default 32).
    #[serde(default = "default_dbpool_max_size")]
    pub dbpool_max_size: u32,
    /// Max seconds to wait for a pool connection ([DAT-06]).
    #[serde(default = "default_dbpool_acquire_timeout_secs")]
    pub dbpool_acquire_timeout_secs: u64,
    /// Max connection lifetime before recycle ([DAT-06]).
    #[serde(default = "default_dbpool_max_lifetime_secs")]
    pub dbpool_max_lifetime_secs: u64,
    /// PostgreSQL statement timeout for pooled connections in milliseconds ([DAT-06]).
    #[serde(default = "default_database_statement_timeout_ms")]
    pub database_statement_timeout_ms: u64,
    /// Maximum allowed query page size ([DAT-09]).
    #[serde(default = "default_query_max_limit")]
    pub query_max_limit: u64,
    /// Upper bound on exact row counts before returning a capped total ([DAT-09]).
    #[serde(default = "default_query_count_max_rows")]
    pub query_count_max_rows: u64,
    /// Deployment API zones; when non-empty, only routes whose `allowed_zones` intersect are registered.
    #[serde(default)]
    pub api_zone: Vec<String>,
    /// Reject command payload properties not declared by the typed payload schema.
    #[serde(default)]
    pub deny_unknown_fields: bool,
    /// Default browser WebSocket wire codec when `Sec-WebSocket-Protocol` is absent (mirrors Python `RTC_WIRE_ENCODING`).
    #[serde(default = "default_rtc_wire_encoding")]
    pub rtc_wire_encoding: String,
    /// Maximum HTTP request body size in bytes ([SUR-02]). Default 256 KiB.
    #[serde(default = "default_request_body_max_bytes")]
    pub request_body_max_bytes: usize,
    /// Maximum multipart upload size in bytes ([SUR-05]). Default 20 MiB.
    #[serde(default = "default_request_upload_max_bytes")]
    pub request_upload_max_bytes: usize,
}

/// Default dbpool max size.
pub fn default_dbpool_max_size() -> u32 {
    32
}

/// Clamp dbpool max size.
pub fn clamp_dbpool_max_size(size: u32) -> u32 {
    size.clamp(1, 128)
}

/// Default dbpool acquire timeout secs.
pub fn default_dbpool_acquire_timeout_secs() -> u64 {
    30
}

/// Default dbpool max lifetime secs.
pub fn default_dbpool_max_lifetime_secs() -> u64 {
    1_800
}

/// Default database statement timeout ms.
pub fn default_database_statement_timeout_ms() -> u64 {
    60_000
}

/// Default request body max bytes.
pub fn default_request_body_max_bytes() -> usize {
    256 * 1024
}

/// Default request upload max bytes.
pub fn default_request_upload_max_bytes() -> usize {
    20 * 1024 * 1024
}

/// Default query max limit.
pub fn default_query_max_limit() -> u64 {
    500
}

/// Default query count max rows.
pub fn default_query_count_max_rows() -> u64 {
    10_000
}

fn default_api_base() -> String {
    crate::api_path::DEFAULT_API_BASE.into()
}

fn default_tenant_access_policy() -> String {
    crate::base::DEFAULT_TENANT_ACCESS_POLICY.into()
}

fn default_tenant_stamp_policy() -> String {
    crate::base::DEFAULT_TENANT_STAMP_POLICY.into()
}

/// Normalize portable PostgreSQL URLs (`postgres://` → `postgresql://`).
pub fn normalize_db_url(url: &str) -> String {
    let trimmed = url.trim();
    if let Some(rest) = trimmed.strip_prefix("postgres://") {
        format!("postgresql://{rest}")
    } else {
        trimmed.to_string()
    }
}

/// Default rtc wire encoding.
pub fn default_rtc_wire_encoding() -> String {
    "json".into()
}

impl Default for RiverbaseConfig {
    fn default() -> Self {
        Self {
            db_url: "postgresql://USER:PASSWORD@127.0.0.1:5432/riverbase_rs".into(),
            bind_addr: "0.0.0.0:8080".into(),
            log_level: "info".into(),
            log_format: LogFormat::default(),
            log_http_requests: false,
            service_mode: "coupled".into(),
            api_base: default_api_base(),
            audit_log: AuditLogConfig::default(),
            auth: AuthConfig::default(),
            bus: BusConfig::default(),
            casbin: CasbinConfig::default(),
            link_token: LinkTokenConfig::default(),
            hook_token: HookTokenConfig::default(),
            media: MediaStorageConfig::default(),
            tenant: String::new(),
            tenant_id: None,
            tenant_access_policy: default_tenant_access_policy(),
            tenant_stamp_policy: default_tenant_stamp_policy(),
            dbpool_max_size: default_dbpool_max_size(),
            dbpool_acquire_timeout_secs: default_dbpool_acquire_timeout_secs(),
            dbpool_max_lifetime_secs: default_dbpool_max_lifetime_secs(),
            database_statement_timeout_ms: default_database_statement_timeout_ms(),
            query_max_limit: default_query_max_limit(),
            query_count_max_rows: default_query_count_max_rows(),
            api_zone: Vec::new(),
            deny_unknown_fields: false,
            rtc_wire_encoding: default_rtc_wire_encoding(),
            request_body_max_bytes: default_request_body_max_bytes(),
            request_upload_max_bytes: default_request_upload_max_bytes(),
        }
    }
}

/// Parse a comma-separated zone list from an environment variable value.
pub fn parse_api_zone_env(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Optional `[riverbase]` table wrapper for TOML files.
#[derive(Debug, Clone, Deserialize, Default)]
struct RiverbaseConfigFile {
    #[serde(default)]
    riverbase: RiverbaseConfig,
}

/// Wire format of a (possibly decrypted) config payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigPayloadFormat {
    /// Toml.
    Toml,
    /// Yaml.
    Yaml,
    /// Json.
    Json,
}

impl ConfigPayloadFormat {
    /// Parse a format hint string (`toml`/`yaml`/`yml`/`json`).
    pub fn parse_hint(hint: &str) -> Option<Self> {
        match hint.trim().to_ascii_lowercase().as_str() {
            "toml" | "tml" => Some(Self::Toml),
            "yaml" | "yml" => Some(Self::Yaml),
            "json" => Some(Self::Json),
            _ => None,
        }
    }

    /// Infer the format from a URI or path suffix; defaults to TOML to match `RiverbaseConfig`.
    pub fn from_uri(uri: &str) -> Self {
        let path = uri.split(['?', '#']).next().unwrap_or(uri);
        let lower = path.to_ascii_lowercase();
        if lower.ends_with(".yaml") || lower.ends_with(".yml") {
            Self::Yaml
        } else if lower.ends_with(".json") {
            Self::Json
        } else {
            Self::Toml
        }
    }
}

impl RiverbaseConfig {
    /// Load from TOML file when present (otherwise defaults), then apply environment overrides.
    pub fn load() -> RiverbaseResult<Self> {
        let mut config = match discover_config_path() {
            Some(path) => Self::from_file(path)?,
            None => Self::default(),
        };
        config.apply_env_overrides();
        config.normalize_paths();
        Ok(config)
    }

    /// Load from environment variables only (no TOML file).
    pub fn from_env() -> RiverbaseResult<Self> {
        let mut config = Self::default();
        config.apply_env_overrides();
        config.normalize_paths();
        Ok(config)
    }

    /// Parse configuration from a TOML file.
    ///
    /// Supports either a top-level `[riverbase]` table or keys at the file root:
    ///
    /// ```toml
    /// [riverbase]
    /// db_url = "postgres://localhost/flrs"
    /// bind_addr = "0.0.0.0:8080"
    /// ```
    pub fn from_file(path: impl AsRef<Path>) -> RiverbaseResult<Self> {
        let path = path.as_ref();
        let contents = std::fs::read_to_string(path).map_err(|e| {
            crate::errors::CFG_180.with_data(format!("read config {}: {e}", path.display()))
        })?;
        Self::from_toml_str(&contents)
    }

    /// Parse configuration from a TOML string.
    pub fn from_toml_str(contents: &str) -> RiverbaseResult<Self> {
        let table: toml::Table = toml::from_str(contents)
            .map_err(|e| crate::errors::CFG_002.with_data(format!("parse config toml: {e}")))?;

        if table.contains_key(CONFIG_SECTION) {
            let wrapped: RiverbaseConfigFile = toml::from_str(contents)
                .map_err(|e| crate::errors::CFG_003.with_data(format!("parse config toml: {e}")))?;
            let mut cfg = wrapped.riverbase;
            cfg.normalize_paths();
            Ok(cfg)
        } else {
            let mut flat: RiverbaseConfig = toml::from_str(contents)
                .map_err(|e| crate::errors::CFG_004.with_data(format!("parse config toml: {e}")))?;
            flat.normalize_paths();
            Ok(flat)
        }
    }

    /// Normalize derived paths (auth base, redirect URIs) from [`api_base`].
    pub fn normalize_paths(&mut self) {
        self.db_url = normalize_db_url(&self.db_url);
        self.api_base = crate::api_path::normalize_api_base(&self.api_base);
        self.auth.normalize_paths(&self.api_base);
        self.resolve_tenant();
    }

    /// Set the raw tenant string and resolve [`Self::tenant_id`].
    pub fn set_tenant(&mut self, tenant: impl Into<String>) {
        self.tenant = tenant.into();
        self.resolve_tenant();
    }

    fn resolve_tenant(&mut self) {
        self.tenant_id = tenant_id_from_config(&self.tenant);
    }

    /// Parse configuration from raw bytes in the given format.
    ///
    /// Like [`from_toml_str`](Self::from_toml_str), this accepts either a top-level
    /// `riverbase` section or root-level keys. SOPS has no TOML mode, so encrypted TOML is
    /// expected to be SOPS *binary* and decrypted back to TOML text before this call.
    pub fn from_bytes(bytes: &[u8], format: ConfigPayloadFormat) -> RiverbaseResult<Self> {
        match format {
            ConfigPayloadFormat::Toml => {
                let text = std::str::from_utf8(bytes).map_err(|e| {
                    crate::errors::CFG_130.with_data(format!("decode config payload: {e}"))
                })?;
                Self::from_toml_str(text)
            }
            ConfigPayloadFormat::Yaml => {
                #[cfg(feature = "yaml")]
                {
                    let value: serde_json::Value = serde_yaml::from_slice(bytes).map_err(|e| {
                        crate::errors::CFG_131.with_data(format!("parse config yaml: {e}"))
                    })?;
                    Self::from_json_value(value)
                }
                #[cfg(not(feature = "yaml"))]
                {
                    let _ = bytes;
                    Err(crate::errors::CFG_185.raise().into())
                }
            }
            ConfigPayloadFormat::Json => {
                let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| {
                    crate::errors::CFG_186.with_data(format!("parse config json: {e}"))
                })?;
                Self::from_json_value(value)
            }
        }
    }

    fn from_json_value(value: serde_json::Value) -> RiverbaseResult<Self> {
        let inner = match value.get(CONFIG_SECTION) {
            Some(section) => section.clone(),
            None => value,
        };
        let mut cfg: RiverbaseConfig = serde_json::from_value(inner)
            .map_err(|e| crate::errors::CFG_187.with_data(format!("deserialize config: {e}")))?;
        cfg.normalize_paths();
        Ok(cfg)
    }

    fn apply_env_overrides(&mut self) {
        if let Ok(value) = std::env::var("RIVERBASE_DB_URL") {
            self.db_url = value;
        } else if let Ok(value) = std::env::var("DB_URL") {
            self.db_url = value;
        }
        if let Ok(value) = std::env::var("RIVERBASE_BIND_ADDR") {
            self.bind_addr = value;
        }
        if let Ok(value) = std::env::var("RIVERBASE_LOG_LEVEL") {
            self.log_level = value;
        }
        if let Ok(value) = std::env::var("RIVERBASE_LOG_FORMAT") {
            self.log_format = LogFormat::parse_hint(&value);
        }
        if let Ok(value) = std::env::var("RIVERBASE_LOG_HTTP_REQUESTS") {
            self.log_http_requests = parse_bool_env(&value);
        }
        if let Ok(value) = std::env::var("RIVERBASE_SERVICE_MODE") {
            self.service_mode = value;
        }
        if let Ok(value) = std::env::var("RIVERBASE_AUDIT_LOG_COMMAND") {
            self.audit_log.command = parse_bool_env(&value);
        }
        if let Ok(value) = std::env::var("RIVERBASE_AUDIT_LOG_CONTEXT") {
            self.audit_log.context = parse_bool_env(&value);
        }
        if let Ok(value) = std::env::var("RIVERBASE_AUDIT_LOG_EVENT") {
            self.audit_log.event = parse_bool_env(&value);
        }
        if let Ok(value) = std::env::var("RIVERBASE_AUDIT_LOG_MESSAGE") {
            self.audit_log.message = parse_bool_env(&value);
        }
        if let Ok(value) = std::env::var("RIVERBASE_AUDIT_LOG_ACTIVITY") {
            self.audit_log.activity = parse_bool_env(&value);
        }
        if let Ok(value) = std::env::var("RIVERBASE_AUDIT_LOG_RESPONSE") {
            self.audit_log.response = parse_bool_env(&value);
        }
        if let Ok(value) = std::env::var("RIVERBASE_AUDIT_LOG_QUERY") {
            self.audit_log.query = parse_bool_env(&value);
        }
        self.auth.apply_env_overrides();
        self.bus.apply_env_overrides();
        if let Ok(value) = std::env::var("RIVERBASE_CASBIN_ENABLED") {
            self.casbin.enabled = parse_bool_env(&value);
        }
        if let Ok(value) = std::env::var("RIVERBASE_CASBIN_OMIT_INACCESSIBLE_OPENAPI") {
            self.casbin.omit_inaccessible_openapi = parse_bool_env(&value);
        }
        if let Ok(value) =
            std::env::var("RIVERBASE_TENANT").or_else(|_| std::env::var("RIVERBASE_REALM"))
        {
            self.tenant = value;
        }
        if let Ok(value) = std::env::var("RIVERBASE_TENANT_ACCESS_POLICY") {
            self.tenant_access_policy = value;
        }
        if let Ok(value) = std::env::var("RIVERBASE_TENANT_STAMP_POLICY") {
            self.tenant_stamp_policy = value;
        }
        self.resolve_tenant();
        if let Ok(value) = std::env::var("RIVERBASE_API_BASE") {
            self.api_base = value;
        }
        if let Ok(value) = std::env::var("RIVERBASE_API_ZONE") {
            self.api_zone = parse_api_zone_env(&value);
        }
        if let Ok(value) = std::env::var("RIVERBASE_DENY_UNKNOWN_FIELDS") {
            self.deny_unknown_fields = parse_bool_env(&value);
        }
        if let Ok(value) = std::env::var("RIVERBASE_RTC_WIRE_ENCODING") {
            self.rtc_wire_encoding = value;
        }
        if let Ok(value) = std::env::var("RIVERBASE_REQUEST_BODY_MAX_BYTES") {
            if let Ok(bytes) = value.parse() {
                self.request_body_max_bytes = bytes;
            }
        }
        if let Ok(value) = std::env::var("RIVERBASE_REQUEST_UPLOAD_MAX_BYTES") {
            if let Ok(bytes) = value.parse() {
                self.request_upload_max_bytes = bytes;
            }
        }
        if let Ok(value) = std::env::var("RIVERBASE_DBPOOL_MAX_SIZE") {
            if let Ok(size) = value.parse() {
                self.dbpool_max_size = size;
            }
        }
        self.dbpool_max_size = clamp_dbpool_max_size(self.dbpool_max_size);
        if let Ok(value) = std::env::var("RIVERBASE_DBPOOL_ACQUIRE_TIMEOUT_SECS") {
            if let Ok(secs) = value.parse() {
                self.dbpool_acquire_timeout_secs = secs;
            }
        }
        if let Ok(value) = std::env::var("RIVERBASE_DBPOOL_MAX_LIFETIME_SECS") {
            if let Ok(secs) = value.parse() {
                self.dbpool_max_lifetime_secs = secs;
            }
        }
        if let Ok(value) = std::env::var("RIVERBASE_DATABASE_STATEMENT_TIMEOUT_MS") {
            if let Ok(ms) = value.parse() {
                self.database_statement_timeout_ms = ms;
            }
        }
        self.link_token = LinkTokenConfig::from_env();
        self.hook_token = HookTokenConfig::from_env();
        self.media.apply_env_overrides();
    }

    /// Service mode.
    pub fn service_mode(&self) -> crate::base::ServiceMode {
        match self.service_mode.as_str() {
            "split" => crate::base::ServiceMode::Split,
            _ => crate::base::ServiceMode::Coupled,
        }
    }
}

/// Resolve config file path: `RIVERBASE_CONFIG`, then `./riverbase.toml`, then `./config/riverbase.toml`.
pub fn discover_config_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var(ENV_CONFIG_PATH) {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }

    [
        PathBuf::from("riverbase.toml"),
        PathBuf::from("config/riverbase.toml"),
    ]
    .into_iter()
    .find(|candidate| candidate.is_file())
}

#[cfg(feature = "cfgfetch")]
impl RiverbaseConfig {
    /// Fetch a config from `spec` (S3/HTTPS/Git/OCI/file), decrypt it with SOPS in
    /// memory when needed, parse it, and apply environment overrides.
    pub async fn load_remote(spec: &crate::cfgfetch::RemoteConfigSpec) -> RiverbaseResult<Self> {
        let plaintext = crate::cfgfetch::fetch_and_decrypt(spec).await?;
        let mut config = Self::from_bytes(&plaintext, spec.format)?;
        config.apply_env_overrides();
        Ok(config)
    }

    /// Load remotely when `RIVERBASE_CONFIG_URI` is set, otherwise fall back to the local
    /// file/env path used by [`load`](Self::load).
    pub async fn load_async() -> RiverbaseResult<Self> {
        match crate::cfgfetch::RemoteConfigSpec::from_env() {
            Some(spec) => Self::load_remote(&spec?).await,
            None => Self::load(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_postgres_database_url() {
        assert_eq!(
            normalize_db_url("postgres://example/db"),
            "postgresql://example/db"
        );
        assert_eq!(
            normalize_db_url("postgresql://example/db"),
            "postgresql://example/db"
        );
    }

    #[test]
    fn database_url_env_fallback() {
        std::env::set_var("DB_URL", "postgres://env/db");
        std::env::remove_var("RIVERBASE_DB_URL");
        let cfg = RiverbaseConfig::from_env().expect("from_env");
        std::env::remove_var("DB_URL");
        assert_eq!(cfg.db_url, "postgresql://env/db");
    }

    #[test]
    fn parses_api_base() {
        let cfg = RiverbaseConfig::from_toml_str(
            r#"
            [riverbase]
            api_base = "/v1"
            "#,
        )
        .expect("parse");

        assert_eq!(cfg.api_base, "/v1");
        assert_eq!(cfg.auth.base_path, "/v1/auth");
    }

    #[test]
    fn parses_log_format() {
        let compact = RiverbaseConfig::from_toml_str(
            r#"
            [riverbase]
            log_format = "compact"
            "#,
        )
        .expect("parse");
        assert_eq!(compact.log_format, LogFormat::Compact);

        let human = RiverbaseConfig::from_toml_str(
            r#"
            [riverbase]
            log_format = "human"
            "#,
        )
        .expect("parse");
        assert_eq!(human.log_format, LogFormat::Compact);

        let json = RiverbaseConfig::from_toml_str(
            r#"
            [riverbase]
            log_format = "json"
            "#,
        )
        .expect("parse");
        assert_eq!(json.log_format, LogFormat::Json);

        let pretty = RiverbaseConfig::from_toml_str(
            r#"
            [riverbase]
            log_format = "pretty"
            "#,
        )
        .expect("parse");
        assert_eq!(pretty.log_format, LogFormat::Pretty);
    }

    #[test]
    fn log_format_defaults_compact() {
        let cfg = RiverbaseConfig::default();
        assert_eq!(cfg.log_format, LogFormat::Compact);
    }

    #[test]
    fn flrs_log_format_env_override() {
        let key = "RIVERBASE_LOG_FORMAT";
        let previous = std::env::var(key).ok();
        std::env::set_var(key, "json");
        let mut cfg = RiverbaseConfig::default();
        cfg.apply_env_overrides();
        if let Some(value) = previous {
            std::env::set_var(key, value);
        } else {
            std::env::remove_var(key);
        }
        assert_eq!(cfg.log_format, LogFormat::Json);
    }

    #[test]
    fn parses_log_http_requests() {
        let cfg = RiverbaseConfig::from_toml_str(
            r#"
            [riverbase]
            log_http_requests = true
            "#,
        )
        .expect("parse");

        assert!(cfg.log_http_requests);
    }

    #[test]
    fn parses_riverbase_section() {
        let cfg = RiverbaseConfig::from_toml_str(
            r#"
            [riverbase]
            db_url = "postgres://example/db"
            bind_addr = "127.0.0.1:9000"
            log_level = "debug"
            service_mode = "split"
            "#,
        )
        .expect("parse");

        assert_eq!(cfg.db_url, "postgresql://example/db");
        assert_eq!(cfg.bind_addr, "127.0.0.1:9000");
        assert_eq!(cfg.log_level, "debug");
        assert_eq!(cfg.service_mode, "split");
    }

    #[test]
    fn parses_flat_root_keys() {
        let cfg = RiverbaseConfig::from_toml_str(
            r#"
            db_url = "postgres://flat/db"
            "#,
        )
        .expect("parse");

        assert_eq!(cfg.db_url, "postgresql://flat/db");
    }

    #[test]
    fn parses_audit_log_section() {
        let cfg = RiverbaseConfig::from_toml_str(
            r#"
            [riverbase]
            [riverbase.audit_log]
            command = true
            event = false
            message = true
            activity = false
            response = true
            query = true
            "#,
        )
        .expect("parse");

        assert!(cfg.audit_log.command);
        assert!(!cfg.audit_log.event);
        assert!(cfg.audit_log.message);
        assert!(!cfg.audit_log.activity);
        assert!(cfg.audit_log.response);
        assert!(cfg.audit_log.query);
    }

    #[test]
    fn loads_from_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("riverbase.toml");
        std::fs::write(
            &path,
            r#"
            [riverbase]
            log_level = "warn"
            "#,
        )
        .expect("write");

        let cfg = RiverbaseConfig::from_file(&path).expect("from_file");
        assert_eq!(cfg.log_level, "warn");
    }

    #[test]
    fn loads_media_fs_root_from_nested_riverbase_media_table() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("riverbase.toml");
        std::fs::write(
            &path,
            r#"
            [riverbase]
            log_level = "info"

            [riverbase.media]
            fs_root = "../../local/media"
            fskey = "file"
            "#,
        )
        .expect("write");

        let cfg = RiverbaseConfig::from_file(&path).expect("from_file");
        assert_eq!(cfg.media.fs_root, "../../local/media");
        assert_eq!(cfg.media.fskey, "file");
        assert_eq!(cfg.media.protocol, "fs");
        assert!(cfg.media.root_path.is_empty());
        assert!(cfg.media.params.is_empty());
    }

    #[test]
    fn loads_media_s3_protocol_and_params_from_toml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("riverbase.toml");
        std::fs::write(
            &path,
            r#"
            [riverbase]
            log_level = "info"

            [riverbase.media]
            fskey = "s3"
            protocol = "s3"
            root_path = "uploads"

            [riverbase.media.params]
            bucket = "my-bucket"
            endpoint = "http://127.0.0.1:9000"
            region = "auto"
            access_key_id = "minio"
            secret_access_key = "minio123"
            "#,
        )
        .expect("write");

        let cfg = RiverbaseConfig::from_file(&path).expect("from_file");
        assert_eq!(cfg.media.fskey, "s3");
        assert_eq!(cfg.media.protocol, "s3");
        assert_eq!(cfg.media.root_path, "uploads");
        assert_eq!(
            cfg.media.params.get("bucket").map(String::as_str),
            Some("my-bucket")
        );
        assert_eq!(
            cfg.media.params.get("endpoint").map(String::as_str),
            Some("http://127.0.0.1:9000")
        );
        assert_eq!(
            cfg.media.params.get("region").map(String::as_str),
            Some("auto")
        );
    }

    #[test]
    fn media_env_overrides_protocol_and_params() {
        let keys = [
            "RIVERBASE_MEDIA_PROTOCOL",
            "RIVERBASE_MEDIA_BUCKET",
            "RIVERBASE_MEDIA_ENDPOINT",
            "RIVERBASE_MEDIA_ROOT_PATH",
        ];
        let previous: Vec<_> = keys.iter().map(|k| (*k, std::env::var(k).ok())).collect();
        std::env::set_var("RIVERBASE_MEDIA_PROTOCOL", "s3");
        std::env::set_var("RIVERBASE_MEDIA_BUCKET", "env-bucket");
        std::env::set_var("RIVERBASE_MEDIA_ENDPOINT", "http://minio:9000");
        std::env::set_var("RIVERBASE_MEDIA_ROOT_PATH", "pfx");

        let mut cfg = RiverbaseConfig::default();
        cfg.apply_env_overrides();
        assert_eq!(cfg.media.protocol, "s3");
        assert_eq!(cfg.media.root_path, "pfx");
        assert_eq!(
            cfg.media.params.get("bucket").map(String::as_str),
            Some("env-bucket")
        );
        assert_eq!(
            cfg.media.params.get("endpoint").map(String::as_str),
            Some("http://minio:9000")
        );

        for (key, value) in previous {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }

    #[cfg(feature = "yaml")]
    #[test]
    fn from_bytes_parses_yaml_riverbase_section() {
        let cfg = RiverbaseConfig::from_bytes(
            b"riverbase:\n  log_level: warn\n  bind_addr: 127.0.0.1:9000\n",
            ConfigPayloadFormat::Yaml,
        )
        .expect("parse yaml");
        assert_eq!(cfg.log_level, "warn");
        assert_eq!(cfg.bind_addr, "127.0.0.1:9000");
    }

    #[test]
    fn from_bytes_parses_json_section_and_flat() {
        let wrapped = RiverbaseConfig::from_bytes(
            br#"{"riverbase":{"service_mode":"split"}}"#,
            ConfigPayloadFormat::Json,
        )
        .expect("parse json section");
        assert_eq!(wrapped.service_mode, "split");

        let flat =
            RiverbaseConfig::from_bytes(br#"{"log_level":"debug"}"#, ConfigPayloadFormat::Json)
                .expect("parse json flat");
        assert_eq!(flat.log_level, "debug");
    }

    #[test]
    fn from_bytes_toml_rejects_invalid_utf8() {
        let err = match RiverbaseConfig::from_bytes(&[0xff, 0xfe, 0x00], ConfigPayloadFormat::Toml) {
            Ok(_) => panic!("expected a UTF-8 decode error"),
            Err(e) => e,
        };
        assert_eq!(err.errcode.as_str(), "CFG-130");
    }

    #[test]
    fn from_bytes_json_rejects_garbage() {
        let err = match RiverbaseConfig::from_bytes(b"not json at all", ConfigPayloadFormat::Json) {
            Ok(_) => panic!("expected a JSON parse error"),
            Err(e) => e,
        };
        assert_eq!(err.errcode.as_str(), "CFG-186");
    }

    #[test]
    fn parses_api_zone_from_toml() {
        let cfg = RiverbaseConfig::from_toml_str(
            r#"
            [riverbase]
            api_zone = ["seller", "staff"]
            "#,
        )
        .expect("parse");

        assert_eq!(
            cfg.api_zone,
            vec!["seller".to_string(), "staff".to_string()]
        );
    }

    #[test]
    fn deny_unknown_fields_defaults_false_and_parses_from_toml() {
        assert!(!RiverbaseConfig::default().deny_unknown_fields);
        let cfg = RiverbaseConfig::from_toml_str(
            r#"
            [riverbase]
            deny_unknown_fields = true
            "#,
        )
        .expect("parse");
        assert!(cfg.deny_unknown_fields);
    }

    #[test]
    fn parse_api_zone_env_splits_and_trims() {
        assert_eq!(
            super::parse_api_zone_env(" seller , staff ,, coordinator "),
            vec![
                "seller".to_string(),
                "staff".to_string(),
                "coordinator".to_string()
            ]
        );
    }

    #[test]
    fn flrs_api_zone_env_override() {
        let key = "RIVERBASE_API_ZONE";
        let previous = std::env::var(key).ok();
        std::env::set_var(key, "seller,coordinator");
        let mut cfg = RiverbaseConfig::default();
        cfg.apply_env_overrides();
        if let Some(value) = previous {
            std::env::set_var(key, value);
        } else {
            std::env::remove_var(key);
        }
        assert_eq!(
            cfg.api_zone,
            vec!["seller".to_string(), "coordinator".to_string()]
        );
    }

    #[test]
    fn parses_casbin_omit_inaccessible_openapi_from_toml() {
        let cfg = RiverbaseConfig::from_toml_str(
            r#"
            [riverbase.casbin]
            enabled = true
            omit_inaccessible_openapi = true
            "#,
        )
        .expect("parse");

        assert!(cfg.casbin.enabled);
        assert!(cfg.casbin.omit_inaccessible_openapi);
    }

    #[test]
    fn casbin_omit_inaccessible_openapi_defaults_false() {
        let cfg = RiverbaseConfig::default();
        assert!(!cfg.casbin.omit_inaccessible_openapi);
    }

    #[test]
    fn flrs_casbin_omit_inaccessible_openapi_env_override() {
        let key = "RIVERBASE_CASBIN_OMIT_INACCESSIBLE_OPENAPI";
        let previous = std::env::var(key).ok();
        std::env::set_var(key, "true");
        let mut cfg = RiverbaseConfig::default();
        cfg.apply_env_overrides();
        if let Some(value) = previous {
            std::env::set_var(key, value);
        } else {
            std::env::remove_var(key);
        }
        assert!(cfg.casbin.omit_inaccessible_openapi);
    }

    #[test]
    fn tenant_uuid_literal_becomes_tenant_id() {
        let id = "00000000-0000-4000-8000-000000000012";
        let cfg = RiverbaseConfig::from_toml_str(&format!(
            r#"
            [riverbase]
            tenant = "{id}"
            "#
        ))
        .expect("parse");
        assert_eq!(cfg.tenant, id);
        assert_eq!(cfg.tenant_id, Some(id.parse().expect("uuid")));
    }

    #[test]
    fn tenant_name_is_hashed_to_tenant_id() {
        let cfg = RiverbaseConfig::from_toml_str(
            r#"
            [riverbase]
            tenant = "abx"
            "#,
        )
        .expect("parse");
        assert_eq!(cfg.tenant, "abx");
        assert_eq!(cfg.tenant_id, Some(crate::base::tenant_uuid("abx")));
    }

    #[test]
    fn realm_alias_hashes_to_tenant_id() {
        let cfg = RiverbaseConfig::from_toml_str(
            r#"
            [riverbase]
            realm = "cpo-client"
            "#,
        )
        .expect("parse");
        assert_eq!(cfg.tenant, "cpo-client");
        assert_eq!(cfg.tenant_id, Some(crate::base::tenant_uuid("cpo-client")));
    }

    #[test]
    fn empty_tenant_has_no_tenant_id() {
        let cfg = RiverbaseConfig::from_toml_str("[riverbase]\n").expect("parse");
        assert!(cfg.tenant.is_empty());
        assert_eq!(cfg.tenant_id, None);
    }

    #[test]
    fn tenant_policies_default_to_profile_organization() {
        let cfg = RiverbaseConfig::from_toml_str("[riverbase]\n").expect("parse");
        assert_eq!(cfg.tenant_access_policy, "profile-organization");
        assert_eq!(cfg.tenant_stamp_policy, "profile-organization");
    }

    #[test]
    fn tenant_policies_from_toml() {
        let cfg = RiverbaseConfig::from_toml_str(
            r#"
            [riverbase]
            tenant_access_policy = "system"
            tenant_stamp_policy = "profile"
            "#,
        )
        .expect("parse");
        assert_eq!(cfg.tenant_access_policy, "system");
        assert_eq!(cfg.tenant_stamp_policy, "profile");
    }
}
