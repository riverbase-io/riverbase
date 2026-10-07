//! RTC WebSocket bridge — browser clients to a subscribe-capable transport.
//!
//! Mirrors Python `riverbase.transport.rtc_bridge` (`RTCBridge`). Connects browser
//! clients to pub/sub channels for real-time bidirectional communication.
//!
//! ```ignore
//! use std::sync::Arc;
//! use riverbase_http::websocket::{RtcBridge, StreamRtcTransport};
//! use riverbase_core::transport::StreamBus;
//!
//! let bus: Arc<dyn StreamBus> = /* PgMessageBus / NATS / Redis */;
//! let rtc = RtcBridge::new("default", "/rtc", bus.clone())
//!     .with_transport(Arc::new(StreamRtcTransport::new(bus)))
//!     .register_builtins()?
//!     .into_router();
//! // merge `rtc` into RiverbaseApp and mount `with_jwt_auth` ahead of it.
//! ```

pub mod bridge;
pub mod bus_transport;
pub mod client;
pub mod codec;
pub mod datadef;
pub mod handler;
pub mod transport;

pub use bridge::{ClientInfo, RtcBridge};
pub use bus_transport::BusRtcTransport;
pub use client::{
    authorize_channel, default_channel_permissions, tenant_segment, AclName, ChannelAclEntry,
    RtcBridgeClient, TransportAction, CHANNEL_PREFIX, CLIENT_PREFIX, PROFILE_PREFIX,
    REQUEST_PREFIX, USER_PREFIX,
};
pub use codec::WireCodec;
pub use datadef::{to_client_message, ClientMessage, TransportMessage};
pub use handler::{register_builtin_handlers, BridgeProxy, MessageRegistry, WsMessageHandler};
pub use transport::{RtcTransport, StreamRtcTransport, Subscription};

#[cfg(test)]
mod tests;
