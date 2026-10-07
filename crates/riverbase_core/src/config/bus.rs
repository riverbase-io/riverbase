//! Message bus configuration ([RUN-01]).

use serde::{Deserialize, Serialize};

use crate::base::RiverbaseResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
/// Bus Kind enumeration.
pub enum BusKind {
    /// Production default when URL is `nats://…` (or unset).
    #[default]
    Nats,
    /// PostgreSQL LISTEN/NOTIFY when URL is `postgresql://…` / `postgres://…`.
    Postgres,
    /// Redis pub/sub when URL is `redis://…` / `rediss://…`.
    Redis,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Bus config structure.
pub struct BusConfig {
    /// Explicit kind override. Prefer URL protocol via [`Self::resolved_kind`].
    #[serde(default)]
    pub kind: BusKind,
    /// Bus URL. Env: `RIVERBASE_BUS_URL` or `BUS_URL`.
    ///
    /// Schemes: `nats://` (default), `redis://` / `rediss://`, `postgresql://` / `postgres://`.
    #[serde(default)]
    pub url: String,
}

impl Default for BusConfig {
    fn default() -> Self {
        Self {
            kind: BusKind::Nats,
            url: String::new(),
        }
    }
}

impl BusConfig {
    /// Apply env overrides.
    pub fn apply_env_overrides(&mut self) {
        if let Ok(value) = std::env::var("RIVERBASE_BUS_URL").or_else(|_| std::env::var("BUS_URL")) {
            if !value.trim().is_empty() {
                self.url = value;
            }
        }
    }

    /// Config `url`, else `RIVERBASE_BUS_URL` / `BUS_URL`, else `nats://127.0.0.1:4222`.
    pub fn resolved_url(&self) -> String {
        if !self.url.trim().is_empty() {
            return self.url.trim().to_string();
        }
        std::env::var("RIVERBASE_BUS_URL")
            .or_else(|_| std::env::var("BUS_URL"))
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "nats://127.0.0.1:4222".into())
    }

    /// Driver from URL scheme (or explicit `[riverbase.bus] kind = "postgres"`).
    pub fn resolved_kind(&self) -> RiverbaseResult<BusKind> {
        if self.kind == BusKind::Postgres && self.url.trim().is_empty() {
            return Ok(BusKind::Postgres);
        }
        kind_from_url(&self.resolved_url())
    }

    /// Validate.
    pub fn validate(&self) -> RiverbaseResult<()> {
        match self.resolved_kind()? {
            BusKind::Postgres => Ok(()),
            BusKind::Nats => {
                #[cfg(feature = "nats-io")]
                {
                    Ok(())
                }
                #[cfg(not(feature = "nats-io"))]
                {
                    Err(crate::errors::CFG_174.raise())
                }
            }
            BusKind::Redis => {
                #[cfg(feature = "redis")]
                {
                    Ok(())
                }
                #[cfg(not(feature = "redis"))]
                {
                    Err(crate::errors::CFG_175.raise())
                }
            }
        }
    }
}

/// Map `scheme://…` to a bus driver.
pub fn kind_from_url(url: &str) -> RiverbaseResult<BusKind> {
    let trimmed = url.trim();
    let scheme = trimmed
        .split_once("://")
        .map(|(s, _)| s)
        .unwrap_or("")
        .to_ascii_lowercase();
    match scheme.as_str() {
        "nats" => Ok(BusKind::Nats),
        "redis" | "rediss" => Ok(BusKind::Redis),
        "postgresql" | "postgres" => Ok(BusKind::Postgres),
        "memory" => Err(crate::errors::CFG_176.raise()),
        "" => Err(crate::errors::CFG_177.raise()),
        other => Err(crate::errors::CFG_178.with_data(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_from_url_protocols() {
        assert_eq!(
            kind_from_url("nats://127.0.0.1:4222").unwrap(),
            BusKind::Nats
        );
        assert_eq!(kind_from_url("NATS://localhost").unwrap(), BusKind::Nats);
        assert_eq!(
            kind_from_url("redis://127.0.0.1:6379").unwrap(),
            BusKind::Redis
        );
        assert_eq!(
            kind_from_url("rediss://secure.example/0").unwrap(),
            BusKind::Redis
        );
        assert_eq!(
            kind_from_url("postgresql://localhost/db").unwrap(),
            BusKind::Postgres
        );
        assert!(kind_from_url("memory://").is_err());
        assert!(kind_from_url("amqp://x").is_err());
        assert!(kind_from_url("not-a-url").is_err());
    }

    #[test]
    fn url_protocol_selects_postgres() {
        let cfg = BusConfig {
            kind: BusKind::Nats,
            url: "postgresql://127.0.0.1/riverbase_rs".into(),
        };
        assert_eq!(cfg.resolved_kind().unwrap(), BusKind::Postgres);
    }

    #[test]
    fn url_protocol_selects_redis() {
        let cfg = BusConfig {
            kind: BusKind::Nats,
            url: "redis://127.0.0.1:6379".into(),
        };
        assert_eq!(cfg.resolved_kind().unwrap(), BusKind::Redis);
        assert_eq!(cfg.resolved_url(), "redis://127.0.0.1:6379");
    }
}
