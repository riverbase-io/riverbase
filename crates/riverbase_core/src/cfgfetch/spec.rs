//! Specification for a remote config source, plus env-driven construction.

use crate::base::RiverbaseResult;
use crate::config::ConfigPayloadFormat;

/// Env var holding the config URI; its presence triggers remote loading.
pub const ENV_CONFIG_URI: &str = "RIVERBASE_CONFIG_URI";
/// Env var overriding the payload format (`toml`/`yaml`/`json`).
pub const ENV_CONFIG_FORMAT: &str = "RIVERBASE_CONFIG_FORMAT";
/// Env var controlling SOPS decryption (`auto`/`force`/`off`).
pub const ENV_CONFIG_SOPS: &str = "RIVERBASE_CONFIG_SOPS";

/// Whether to attempt SOPS decryption of the fetched payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SopsMode {
    /// Decrypt only when the payload looks SOPS-encrypted (default).
    Auto,
    /// Always run SOPS; fail if the binary is missing or the payload is not encrypted.
    Force,
    /// Never decrypt; use the payload as fetched.
    Off,
}

impl SopsMode {
    fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "force" | "on" => Self::Force,
            "0" | "false" | "no" | "off" => Self::Off,
            _ => Self::Auto,
        }
    }
}

/// Where and how to obtain a [`RiverbaseConfig`](crate::config::RiverbaseConfig).
#[derive(Debug, Clone)]
pub struct RemoteConfigSpec {
    /// Source URI, e.g. `s3://bucket/key`, `https://host/path`, `git+https://...`, `oci://...`.
    pub uri: String,
    /// Format used to parse the decrypted payload.
    pub format: ConfigPayloadFormat,
    /// SOPS decryption behavior.
    pub sops: SopsMode,
}

impl RemoteConfigSpec {
    /// Build a spec for `uri`, inferring the format from its suffix and defaulting
    /// SOPS handling to [`SopsMode::Auto`].
    pub fn new(uri: impl Into<String>) -> Self {
        let uri = uri.into();
        let format = ConfigPayloadFormat::from_uri(&uri);
        Self {
            uri,
            format,
            sops: SopsMode::Auto,
        }
    }

    /// Override the payload format.
    pub fn with_format(mut self, format: ConfigPayloadFormat) -> Self {
        self.format = format;
        self
    }

    /// Override the SOPS decryption mode.
    pub fn with_sops(mut self, sops: SopsMode) -> Self {
        self.sops = sops;
        self
    }

    /// Build a spec from the environment. Returns `None` when `RIVERBASE_CONFIG_URI` is
    /// unset or empty, signalling that local loading should be used instead.
    pub fn from_env() -> Option<RiverbaseResult<Self>> {
        let uri = std::env::var(ENV_CONFIG_URI).ok()?;
        if uri.trim().is_empty() {
            return None;
        }
        Some(Self::from_env_with_uri(uri))
    }

    fn from_env_with_uri(uri: String) -> RiverbaseResult<Self> {
        let mut spec = Self::new(uri);
        if let Ok(fmt) = std::env::var(ENV_CONFIG_FORMAT) {
            if !fmt.trim().is_empty() {
                spec.format = ConfigPayloadFormat::parse_hint(&fmt).ok_or_else(|| {
                    crate::errors::CFG_171.with_data(format!(
                        "{ENV_CONFIG_FORMAT}={fmt} (expected toml|yaml|json)"
                    ))
                })?;
            }
        }
        if let Ok(sops) = std::env::var(ENV_CONFIG_SOPS) {
            spec.sops = SopsMode::parse(&sops);
        }
        Ok(spec)
    }
}
