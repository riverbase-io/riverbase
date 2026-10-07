//! Integration tests for RTC transport and message handlers.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::Stream;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::sync::{broadcast, Mutex};
use tokio::time::timeout;

use crate::auth::Principal;
use crate::base::RiverbaseResult;
use crate::command::MessageBus;
use crate::transport::StreamBus;
use crate::websocket::client::{
    authorize_channel, default_channel_permissions, RtcBridgeClient, TransportAction,
};
use crate::websocket::datadef::to_client_message;
use crate::websocket::handler::{
    handle_client_message, register_builtin_handlers, BridgeProxy, MessageRegistry,
};
use crate::websocket::transport::{RtcTransport, StreamRtcTransport};
use crate::websocket::ClientMessage;

/// In-process [`StreamBus`] for unit tests (not for production).
#[derive(Clone, Default)]
struct LocalStreamBus {
    channels: Arc<Mutex<HashMap<String, broadcast::Sender<Value>>>>,
}

impl LocalStreamBus {
    fn new() -> Self {
        Self::default()
    }

    async fn sender(&self, topic: &str) -> broadcast::Sender<Value> {
        let mut map = self.channels.lock().await;
        map.entry(topic.to_string())
            .or_insert_with(|| broadcast::channel(64).0)
            .clone()
    }
}

#[async_trait]
impl MessageBus for LocalStreamBus {
    async fn publish(&self, topic: &str, payload: Value) -> RiverbaseResult<()> {
        let tx = self.sender(topic).await;
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
        let tx = self.sender(topic).await;
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

fn principal(sub: &str) -> Principal {
    Principal {
        sub: sub.into(),
        preferred_username: None,
        email: None,
        roles: vec![],
        iam_roles: vec![],
        claims: json!({}),
    }
}

#[tokio::test]
async fn cross_user_publish_is_denied() {
    let p_a = principal("user-a");
    let client_a = RtcBridgeClient::new(p_a, default_channel_permissions(&principal("user-a")));
    assert!(authorize_channel(&client_a, "ws.user._.user-b", TransportAction::Publish).is_err());
}

#[tokio::test]
async fn ping_returns_resp_and_publishes_transport_ping() {
    let bus = Arc::new(LocalStreamBus::new()) as Arc<dyn StreamBus>;
    let transport = Arc::new(StreamRtcTransport::new(bus));
    let proxy = BridgeProxy::new(transport.clone());
    let mut registry = MessageRegistry::new();
    register_builtin_handlers(&mut registry).unwrap();

    let p = principal("user-x");
    let client = RtcBridgeClient::new(p.clone(), default_channel_permissions(&p));
    let user_channel = client.user_transport_channel();

    let (fwd_tx, mut fwd_rx) = mpsc::unbounded_channel();
    let sub = transport.subscribe(&user_channel).await.unwrap();
    tokio::spawn(async move {
        while let Some(tm) = sub.recv().await {
            let _ = fwd_tx.send(to_client_message(tm));
        }
    });

    let msg = ClientMessage::new("ping", "t", json!({})).with_txid("tx-1");
    let replies = handle_client_message(&registry, &proxy, &client, msg).await;
    assert_eq!(replies[0].msg_type, "ws.resp.ping");

    let transport_ping = timeout(Duration::from_secs(1), fwd_rx.recv())
        .await
        .expect("timed out")
        .expect("closed");
    assert_eq!(transport_ping.msg_type, "ws.ping.transport");
}
