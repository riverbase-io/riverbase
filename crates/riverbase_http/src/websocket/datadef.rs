//! Wire and transport message structs for the RTC WebSocket bridge.
//!
//! Mirrors Python `riverbase.transport.datadef`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::base::RiverbaseError;

/// Client-visible WebSocket envelope (`RTC_WIRE_ENCODING`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClientMessage {
    #[serde(default)]
    /// Txid.
    pub txid: String,
    #[serde(default, rename = "type")]
    /// Msg type.
    pub msg_type: String,
    #[serde(default)]
    /// Topic.
    pub topic: String,
    #[serde(default)]
    /// Command or event payload.
    pub payload: Value,
    #[serde(default = "default_timestamp")]
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
}

fn default_timestamp() -> DateTime<Utc> {
    Utc::now()
}

impl ClientMessage {
    /// Construct a new value.
    pub fn new(msg_type: impl Into<String>, topic: impl Into<String>, payload: Value) -> Self {
        Self {
            msg_type: msg_type.into(),
            topic: topic.into(),
            payload,
            ..Default::default()
        }
    }

    /// Set txid and return self.
    pub fn with_txid(mut self, txid: impl Into<String>) -> Self {
        self.txid = txid.into();
        self
    }
}

/// Full broker envelope for pub/sub (`RTC_TRANSPORT_ENCODING`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransportMessage {
    #[serde(default)]
    /// Txid.
    pub txid: String,
    #[serde(default, rename = "type")]
    /// Msg type.
    pub msg_type: String,
    #[serde(default)]
    /// Topic.
    pub topic: String,
    #[serde(default)]
    /// Command or event payload.
    pub payload: Value,
    #[serde(default = "default_timestamp")]
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
    #[serde(default)]
    /// Channel.
    pub channel: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Sender.
    pub sender: Option<String>,
    #[serde(default)]
    /// Source.
    pub source: String,
    #[serde(default)]
    /// Header.
    pub header: Value,
}

impl TransportMessage {
    /// Construct a new value.
    pub fn new(channel: impl Into<String>, msg_type: impl Into<String>) -> Self {
        Self {
            channel: channel.into(),
            msg_type: msg_type.into(),
            ..Default::default()
        }
    }
}

/// Strip transport-only fields for delivery to browser clients.
pub fn to_client_message(msg: TransportMessage) -> ClientMessage {
    ClientMessage {
        txid: msg.txid,
        msg_type: msg.msg_type,
        topic: msg.topic,
        payload: msg.payload,
        timestamp: msg.timestamp,
    }
}

/// Decode a published bus payload into a transport message.
pub fn transport_from_value(value: Value) -> TransportMessage {
    serde_json::from_value(value.clone()).unwrap_or_else(|_| TransportMessage {
        payload: value,
        ..Default::default()
    })
}

/// Encode a transport message for the message bus.
pub fn transport_to_value(msg: &TransportMessage) -> Value {
    serde_json::to_value(msg).unwrap_or_else(|_| json!({}))
}

/// Channel permission error.
pub fn channel_permission_error(action: &str, channel: &str) -> RiverbaseError {
    crate::errors::T00_102.with_data(json!({ "action": action, "channel": channel }))
}

/// Client message permission error.
pub fn client_message_permission_error(errmesg: impl Into<String>) -> RiverbaseError {
    crate::errors::T00_103.with_data(json!({ "detail": errmesg.into() }))
}
