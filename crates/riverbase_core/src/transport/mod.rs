//! Transport bus and RPC dispatch adapters.

/// Bus; module.
pub mod bus;
/// Dispatch; module.
pub mod dispatch;
/// Nats; module.
#[cfg(feature = "nats-io")]
pub mod nats;
pub mod postgres;
/// Redis; module.
#[cfg(feature = "redis")]
pub mod redis;

pub mod connect;
pub mod stream_bus;

#[cfg(feature = "nats-io")]
pub use bus::NatsMessageBus;
#[cfg(feature = "redis")]
pub use bus::RedisMessageBus;
pub use connect::connect_stream_bus;
pub use dispatch::{CommandDispatchActor, QueryDispatchActor, TransportEnvelope};
pub use postgres::PgMessageBus;
pub use stream_bus::StreamBus;
