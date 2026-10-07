//! Wire codecs for RTC WebSocket frames (JSON text, CBOR binary).
//!
//! Mirrors Python `riverbase.transport.codec`.

use axum::extract::ws::{Message, Utf8Bytes};
use ciborium::{de::from_reader, ser::into_writer};
use serde_json::Value;

use crate::base::RiverbaseResult;

use super::datadef::ClientMessage;

/// Encoding used on the browser WebSocket (`RTC_WIRE_ENCODING`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WireCodec {
    #[default]
    /// Json.
    Json,
    /// Cbor.
    Cbor,
}

impl WireCodec {
    /// Build from encoding.
    pub fn from_encoding(name: &str) -> Option<Self> {
        match name {
            "json" => Some(Self::Json),
            "cbor" => Some(Self::Cbor),
            _ => None,
        }
    }

    /// Encoding.
    pub fn encoding(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Cbor => "cbor",
        }
    }

    /// Uses text frames.
    pub fn uses_text_frames(self) -> bool {
        matches!(self, Self::Json)
    }

    /// Encode.
    pub fn encode(&self, message: &ClientMessage) -> RiverbaseResult<Message> {
        match self {
            Self::Json => {
                let text = serde_json::to_string(message)
                    .map_err(|e| crate::errors::RTC_001.with_data(e.to_string()))?;
                Ok(Message::Text(Utf8Bytes::from(text)))
            }
            Self::Cbor => {
                let mut buf = Vec::new();
                into_writer(message, &mut buf)
                    .map_err(|e| crate::errors::RTC_002.with_data(e.to_string()))?;
                Ok(Message::Binary(buf.into()))
            }
        }
    }

    /// Decode.
    pub fn decode(&self, frame: Message) -> RiverbaseResult<ClientMessage> {
        match (self, frame) {
            (Self::Json, Message::Text(text)) => serde_json::from_str(text.as_str())
                .map_err(|e| crate::errors::RTC_003.with_data(e.to_string())),
            (Self::Cbor, Message::Binary(bytes)) => from_reader(bytes.as_ref())
                .map_err(|e| crate::errors::RTC_004.with_data(e.to_string())),
            (Self::Json, Message::Binary(_)) => Err(crate::errors::RTC_005.with_data(Value::Null)),
            (Self::Cbor, Message::Text(_)) => Err(crate::errors::RTC_006.with_data(Value::Null)),
            (_, Message::Ping(_) | Message::Pong(_)) => {
                Err(crate::errors::RTC_007.with_data(Value::Null))
            }
            (_, Message::Close(_)) => Err(crate::errors::RTC_008.with_data(Value::Null)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_roundtrip() {
        let msg = ClientMessage::new("ping", "t1", json!({"x": 1})).with_txid("abc");
        let codec = WireCodec::Json;
        let frame = codec.encode(&msg).unwrap();
        let back = codec.decode(frame).unwrap();
        assert_eq!(back.msg_type, "ping");
        assert_eq!(back.txid, "abc");
    }

    #[test]
    fn cbor_roundtrip() {
        let msg = ClientMessage::new("echo", "t2", json!({}));
        let codec = WireCodec::Cbor;
        let frame = codec.encode(&msg).unwrap();
        let back = codec.decode(frame).unwrap();
        assert_eq!(back.msg_type, "echo");
    }
}
