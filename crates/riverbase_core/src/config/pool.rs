//! Postgres pool tuning ([DAT-06]).

use std::time::Duration;

use serde::Deserialize;

/// Async Postgres pool options applied by [`crate::datastore::postgres::establish_dbpool_with_options`].
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct DatabasePoolConfig {
    /// Max connections in the async pool (see [`super::RiverbaseConfig::dbpool_max_size`]).
    pub max_size: u32,
    /// Max time to wait for a free connection before failing acquire (seconds).
    pub acquire_timeout_secs: u64,
    /// Max connection age before recycle (seconds). Applied via deadpool recycle timeout.
    pub max_lifetime_secs: u64,
    /// PostgreSQL `statement_timeout` applied on each new connection (milliseconds).
    pub statement_timeout_ms: u64,
    /// Minimum connections to open eagerly after the pool is built.
    pub min_connections: u32,
}

impl Default for DatabasePoolConfig {
    fn default() -> Self {
        Self {
            max_size: super::default_dbpool_max_size(),
            acquire_timeout_secs: 30,
            max_lifetime_secs: 1_800,
            statement_timeout_ms: 60_000,
            min_connections: 2,
        }
    }
}

impl DatabasePoolConfig {
    /// Build from riverbase config.
    pub fn from_riverbase_config(config: &super::RiverbaseConfig) -> Self {
        Self {
            max_size: config.dbpool_max_size,
            acquire_timeout_secs: config.dbpool_acquire_timeout_secs,
            max_lifetime_secs: config.dbpool_max_lifetime_secs,
            statement_timeout_ms: config.database_statement_timeout_ms,
            min_connections: Self::default().min_connections,
        }
    }

    /// Acquire timeout.
    pub fn acquire_timeout(&self) -> Duration {
        Duration::from_secs(self.acquire_timeout_secs.max(1))
    }

    /// Max lifetime.
    pub fn max_lifetime(&self) -> Duration {
        Duration::from_secs(self.max_lifetime_secs.max(60))
    }

    /// Statement timeout ms.
    pub fn statement_timeout_ms(&self) -> u64 {
        self.statement_timeout_ms.max(1_000)
    }

    /// Connections to open eagerly after the pool is built.
    pub fn min_connections(&self) -> usize {
        super::clamp_dbpool_max_size(self.min_connections.min(self.max_size)) as usize
    }
}
