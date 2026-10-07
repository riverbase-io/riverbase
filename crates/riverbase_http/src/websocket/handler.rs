//! WebSocket message handler registry and built-in handlers.
//!
//! Mirrors `RTCBridge.on_message` and built-in `ping` / `echo` / `sendusr` / `sendchan`.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::base::RiverbaseResult;

use super::client::{authorize_channel, RtcBridgeClient, TransportAction, CHANNEL_PREFIX};
use super::datadef::{ClientMessage, TransportMessage};
use super::transport::RtcTransport;

/// Proxy passed to handlers for publish/subscribe (mirrors `RTCBridgeProxy`).
pub struct BridgeProxy {
    transport: Arc<dyn RtcTransport>,
}

impl BridgeProxy {
    /// Construct a new value.
    pub fn new(transport: Arc<dyn RtcTransport>) -> Self {
        Self { transport }
    }

    /// Publish.
    pub async fn publish(
        &self,
        client: Option<&RtcBridgeClient>,
        message: TransportMessage,
    ) -> RiverbaseResult<()> {
        if let Some(cli) = client {
            authorize_channel(cli, &message.channel, TransportAction::Publish)?;
        }
        self.transport.publish(message).await
    }
}

#[async_trait]
/// Ws message handler trait.
pub trait WsMessageHandler: Send + Sync {
    /// Handle.
    async fn handle(
        &self,
        proxy: &BridgeProxy,
        client: &RtcBridgeClient,
        message: ClientMessage,
    ) -> RiverbaseResult<Vec<ClientMessage>>;

    /// Model schema.
    fn model_schema(&self) -> Option<Value> {
        None
    }

    /// Description.
    fn description(&self) -> &'static str {
        ""
    }
}

/// Message handler entry structure.
pub struct MessageHandlerEntry {
    /// Name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Handler.
    pub handler: Arc<dyn WsMessageHandler>,
}

/// Message registry structure.
pub struct MessageRegistry {
    handlers: HashMap<String, MessageHandlerEntry>,
}

impl Default for MessageRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageRegistry {
    /// Construct a new value.
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register.
    pub fn register(
        &mut self,
        msgtype: &str,
        name: impl Into<String>,
        description: impl Into<String>,
        handler: Arc<dyn WsMessageHandler>,
    ) -> RiverbaseResult<()> {
        if !is_valid_msgtype(msgtype) {
            return Err(crate::errors::RTC_020.with_data(json!({ "type": msgtype })));
        }
        if self.handlers.contains_key(msgtype) {
            return Err(crate::errors::RTC_021.with_data(json!({ "type": msgtype })));
        }
        self.handlers.insert(
            msgtype.to_string(),
            MessageHandlerEntry {
                name: name.into(),
                description: description.into(),
                handler,
            },
        );
        Ok(())
    }

    /// Get.
    pub fn get(&self, msgtype: &str) -> Option<&MessageHandlerEntry> {
        self.handlers.get(msgtype)
    }

    /// Iter.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &MessageHandlerEntry)> {
        self.handlers.iter()
    }
}

fn is_valid_msgtype(msgtype: &str) -> bool {
    if msgtype.is_empty() {
        return false;
    }
    msgtype
        .split(':')
        .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
}

fn ws_error(txid: &str, topic: &str, errcode: &str, errmsg: impl Into<String>) -> ClientMessage {
    ClientMessage::new(
        "ws.error",
        topic,
        json!({
            "errmsg": errmsg.into(),
            "errcode": errcode,
        }),
    )
    .with_txid(txid)
}

/// Dispatch a client message through the registry; returns replies (no socket I/O).
pub async fn handle_client_message(
    registry: &MessageRegistry,
    proxy: &BridgeProxy,
    client: &RtcBridgeClient,
    message: ClientMessage,
) -> Vec<ClientMessage> {
    let txid = message.txid.clone();
    let topic = message.topic.clone();

    let Some(entry) = registry.get(&message.msg_type) else {
        return vec![ws_error(
            &txid,
            &topic,
            "WS104",
            format!("Unknown message type: {}", message.msg_type),
        )];
    };

    match entry.handler.handle(proxy, client, message).await {
        Ok(replies) => replies,
        Err(e) if e.http_status == 403 => {
            let mut payload = json!({
                "errmsg": e.errmesg,
                "errcode": e.errcode.as_str(),
            });
            if !e.errdata.is_null() {
                payload["errdata"] = e.errdata.clone();
            }
            vec![ClientMessage::new("ws.error", topic, payload).with_txid(txid)]
        }
        Err(_e) => vec![ws_error(
            &txid,
            &topic,
            "WS102",
            "Unable to handle message.",
        )],
    }
}

// ── Built-in handlers ───────────────────────────────────────────────────────

struct PingHandler;

#[async_trait]
impl WsMessageHandler for PingHandler {
    async fn handle(
        &self,
        proxy: &BridgeProxy,
        client: &RtcBridgeClient,
        message: ClientMessage,
    ) -> RiverbaseResult<Vec<ClientMessage>> {
        let reply = ClientMessage::new("ws.resp.ping", &message.topic, message.payload.clone())
            .with_txid(&message.txid);
        let transport_msg = TransportMessage {
            channel: client.user_transport_channel(),
            txid: message.txid.clone(),
            msg_type: "ws.ping.transport".into(),
            topic: client.user_id.clone(),
            payload: message.payload,
            ..Default::default()
        };
        proxy.publish(Some(client), transport_msg).await?;
        Ok(vec![reply])
    }

    fn description(&self) -> &'static str {
        "Reply with ws.resp.ping and publish ws.ping.transport on the user channel."
    }
}

struct EchoHandler;

#[async_trait]
impl WsMessageHandler for EchoHandler {
    async fn handle(
        &self,
        _proxy: &BridgeProxy,
        _client: &RtcBridgeClient,
        message: ClientMessage,
    ) -> RiverbaseResult<Vec<ClientMessage>> {
        Ok(vec![ClientMessage::new(
            "ws.resp.echo",
            &message.topic,
            message.payload,
        )
        .with_txid(&message.txid)])
    }

    fn description(&self) -> &'static str {
        "Echo the request payload back to this client."
    }
}

#[derive(Debug, Deserialize)]
struct BroadcastMessage {
    #[allow(dead_code)]
    message: String,
    #[allow(dead_code)]
    data: Value,
}

struct SendChannelHandler;

#[async_trait]
impl WsMessageHandler for SendChannelHandler {
    async fn handle(
        &self,
        proxy: &BridgeProxy,
        client: &RtcBridgeClient,
        message: ClientMessage,
    ) -> RiverbaseResult<Vec<ClientMessage>> {
        let _model: BroadcastMessage = serde_json::from_value(message.payload.clone())
            .map_err(|e| crate::errors::RTC_022.with_data(e.to_string()))?;
        let channel = format!("{CHANNEL_PREFIX}{}", message.topic);
        proxy
            .publish(
                Some(client),
                TransportMessage {
                    channel,
                    txid: message.txid.clone(),
                    msg_type: message.msg_type.clone(),
                    topic: message.topic.clone(),
                    payload: message.payload,
                    ..Default::default()
                },
            )
            .await?;
        Ok(vec![ClientMessage::new(
            "ws.resp.sendchan",
            &message.topic,
            json!({ "status": "success" }),
        )
        .with_txid(&message.txid)])
    }

    fn description(&self) -> &'static str {
        "Send a message to a specific transport channel."
    }
}

/// Register built-in message handlers (`ping`, `echo`, `sendchan`).
///
/// `sendusr` was removed (SUR-01): cross-user publish via a wildcard ACL is no longer supported.
pub fn register_builtin_handlers(registry: &mut MessageRegistry) -> RiverbaseResult<()> {
    registry.register(
        "ping",
        "ping",
        "Reply with ws.resp.ping and publish ws.ping.transport on the user channel.",
        Arc::new(PingHandler),
    )?;
    registry.register(
        "echo",
        "echo",
        "Echo the request payload back to this client.",
        Arc::new(EchoHandler),
    )?;
    registry.register(
        "sendchan",
        "sendchan",
        "Send a message to a specific transport channel.",
        Arc::new(SendChannelHandler),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::sync::Arc;

    use async_trait::async_trait;
    use futures_util::Stream;
    use serde_json::{json, Value};
    use tokio::sync::{broadcast, Mutex};

    use super::*;
    use crate::auth::Principal;
    use crate::base::RiverbaseResult;
    use crate::command::MessageBus;
    use crate::transport::StreamBus;
    use crate::websocket::client::{default_channel_permissions, RtcBridgeClient};
    use crate::websocket::transport::StreamRtcTransport;

    #[derive(Clone, Default)]
    struct LocalStreamBus {
        channels: Arc<Mutex<HashMap<String, broadcast::Sender<Value>>>>,
    }

    #[async_trait]
    impl MessageBus for LocalStreamBus {
        async fn publish(&self, topic: &str, payload: Value) -> RiverbaseResult<()> {
            let mut map = self.channels.lock().await;
            let tx = map
                .entry(topic.to_string())
                .or_insert_with(|| broadcast::channel(64).0);
            let _ = tx.send(payload);
            Ok(())
        }
    }

    #[async_trait]
    impl StreamBus for LocalStreamBus {
        async fn subscribe_values(
            &self,
            topic: &str,
        ) -> RiverbaseResult<Pin<Box<dyn Stream<Item = Value> + Send>>> {
            let mut map = self.channels.lock().await;
            let tx = map
                .entry(topic.to_string())
                .or_insert_with(|| broadcast::channel(64).0)
                .clone();
            let mut rx = tx.subscribe();
            Ok(Box::pin(async_stream::stream! {
                loop {
                    match rx.recv().await {
                        Ok(value) => yield value,
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            }))
        }
    }

    fn test_client() -> RtcBridgeClient {
        let p = Principal {
            sub: "user-a".into(),
            preferred_username: None,
            email: None,
            roles: vec![],
            iam_roles: vec![],
            claims: json!({}),
        };
        RtcBridgeClient::new(p.clone(), default_channel_permissions(&p))
    }

    #[tokio::test]
    async fn echo_handler_replies() {
        let bus = Arc::new(LocalStreamBus::default()) as Arc<dyn StreamBus>;
        let transport = Arc::new(StreamRtcTransport::new(bus));
        let proxy = BridgeProxy::new(transport);
        let mut reg = MessageRegistry::new();
        register_builtin_handlers(&mut reg).unwrap();
        let client = test_client();
        let msg = ClientMessage::new("echo", "t", json!({"k": 1}));
        let replies = handle_client_message(&reg, &proxy, &client, msg).await;
        assert_eq!(replies[0].msg_type, "ws.resp.echo");
    }
}
