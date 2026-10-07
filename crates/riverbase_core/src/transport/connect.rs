//! Connect configured message bus backends ([RUN-01]).

use std::sync::Arc;

use crate::base::RiverbaseResult;
use crate::config::{BusKind, RiverbaseConfig};

use super::postgres::PgMessageBus;
use super::stream_bus::StreamBus;

/// Connect stream bus.
pub async fn connect_stream_bus(config: &RiverbaseConfig) -> RiverbaseResult<Arc<dyn StreamBus>> {
    config.bus.validate()?;
    let kind = config.bus.resolved_kind()?;
    match kind {
        BusKind::Postgres => {
            let url = if config.bus.url.trim().is_empty() {
                config.db_url.clone()
            } else {
                config.bus.resolved_url()
            };
            let bus = PgMessageBus::connect(&url).await?;
            Ok(Arc::new(bus))
        }
        BusKind::Nats => {
            #[cfg(feature = "nats-io")]
            {
                let url = config.bus.resolved_url();
                let bus = super::nats::NatsMessageBus::connect(&url).await?;
                return Ok(Arc::new(bus));
            }
            #[cfg(not(feature = "nats-io"))]
            {
                let _ = config;
                Err(missing_bus_feature("nats-io", "nats://"))
            }
        }
        BusKind::Redis => {
            #[cfg(feature = "redis")]
            {
                let url = config.bus.resolved_url();
                let bus = super::redis::RedisMessageBus::connect(&url).await?;
                return Ok(Arc::new(bus));
            }
            #[cfg(not(feature = "redis"))]
            {
                let _ = config;
                Err(missing_bus_feature("redis", "redis://"))
            }
        }
    }
}

#[allow(dead_code)]
fn missing_bus_feature(feature: &str, _scheme: &str) -> crate::base::RiverbaseError {
    crate::errors::CFG_151.with_hint(format!(
        "Rebuild with riverbase_core feature `{feature}`, or set [riverbase.bus] kind = \"postgres\" / a postgresql:// URL."
    ))
}
