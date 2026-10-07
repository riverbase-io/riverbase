//! Subscribe-capable RTC transport abstraction.
//!
//! Mirrors Python `ServiceTransport` pub/sub used by `RTCBridge`.

use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::Value;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::Mutex;

use crate::base::RiverbaseResult;
use crate::transport::StreamBus;

use super::datadef::{transport_from_value, transport_to_value, TransportMessage};

/// Active subscription; dropping unsubscribes from the bridge's perspective.
pub struct Subscription {
    rx: Mutex<tokio::sync::broadcast::Receiver<serde_json::Value>>,
}

impl Subscription {
    pub(crate) fn from_receiver(rx: tokio::sync::broadcast::Receiver<serde_json::Value>) -> Self {
        Self { rx: Mutex::new(rx) }
    }

    /// Recv.
    pub async fn recv(&self) -> Option<TransportMessage> {
        loop {
            let mut guard = self.rx.lock().await;
            match guard.recv().await {
                Ok(value) => return Some(transport_from_value(value)),
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return None,
            }
        }
    }
}

#[async_trait]
/// Rtc transport trait.
pub trait RtcTransport: Send + Sync {
    /// Publish.
    async fn publish(&self, message: TransportMessage) -> RiverbaseResult<()>;

    /// Subscribe.
    async fn subscribe(&self, channel: &str) -> RiverbaseResult<Subscription>;
}

/// RTC transport backed by any [`StreamBus`] (Postgres NOTIFY, NATS, Redis, …).
#[derive(Clone)]
pub struct StreamRtcTransport {
    bus: Arc<dyn StreamBus>,
}

impl StreamRtcTransport {
    /// Construct a new value.
    pub fn new(bus: Arc<dyn StreamBus>) -> Self {
        Self { bus }
    }
}

type ValueStream = Pin<Box<dyn futures_util::Stream<Item = Value> + Send>>;

#[async_trait]
impl RtcTransport for StreamRtcTransport {
    async fn publish(&self, message: TransportMessage) -> RiverbaseResult<()> {
        let channel = message.channel.clone();
        let payload = transport_to_value(&message);
        self.bus.publish(&channel, payload).await
    }

    async fn subscribe(&self, channel: &str) -> RiverbaseResult<Subscription> {
        let mut stream: ValueStream = self.bus.subscribe_values(channel).await?;
        let (tx, rx) = tokio::sync::broadcast::channel(64);
        tokio::spawn(async move {
            while let Some(value) = stream.next().await {
                let _ = tx.send(value);
            }
        });
        Ok(Subscription::from_receiver(rx))
    }
}
