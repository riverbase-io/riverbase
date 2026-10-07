use serde::{Deserialize, Serialize};

/// Casbin activity authorization.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct CasbinConfig {
    /// Enabled.
    pub enabled: bool,
    /// When true, `/api/openapi.json` omits operations the requesting subject cannot access.
    #[serde(default)]
    pub omit_inaccessible_openapi: bool,
    /// Inline Casbin model CONF. Empty → framework default at startup.
    #[serde(default)]
    pub model: String,
    /// Inline policy CSV (`p,` / `g,` rows). Empty → deny-all until policies are configured.
    #[serde(default)]
    pub policy: String,
}
