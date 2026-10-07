use std::sync::Arc;

#[cfg(feature = "nats-io")]
use futures_util::StreamExt;
#[cfg(feature = "nats-io")]
use serde_json::Value;
#[cfg(feature = "nats-io")]
use std::future::Future;
use tracing::info;

use riverbase_core::base::RiverbaseResult;
use riverbase_core::transport::StreamBus;

/// Drain a few messages from any [`StreamBus`] (smoke / side-effect helper).
pub struct MessageConsumer {
    bus: Arc<dyn StreamBus>,
    topic: String,
}

impl MessageConsumer {
    pub fn new(bus: Arc<dyn StreamBus>, topic: impl Into<String>) -> Self {
        Self {
            bus,
            topic: topic.into(),
        }
    }

    pub async fn drain_once(&self) -> RiverbaseResult<usize> {
        let mut stream = self.bus.subscribe_values(&self.topic).await?;
        let mut count = 0usize;
        // Non-blocking: take whatever is immediately available after a brief wait is not
        // possible without timeout; take at most one buffered notify if present via try_next.
        if let Some(payload) = futures_util::StreamExt::next(&mut stream).await {
            info!(topic = %self.topic, ?payload, "worker consumed message");
            count = 1;
        }
        Ok(count)
    }
}

/// Consume integration messages from NATS (used by external worker processes).
#[cfg(feature = "nats-io")]
pub struct NatsMessageConsumer {
    bus: riverbase_core::transport::NatsMessageBus,
}

#[cfg(feature = "nats-io")]
impl NatsMessageConsumer {
    pub fn new(bus: riverbase_core::transport::NatsMessageBus) -> Self {
        Self { bus }
    }

    /// Block and invoke `handler` for each message on `subject`.
    pub async fn run<F, Fut>(&self, subject: &str, mut handler: F) -> RiverbaseResult<()>
    where
        F: FnMut(String, Value) -> Fut,
        Fut: Future<Output = RiverbaseResult<()>>,
    {
        let mut sub = self.bus.subscribe(subject).await?;
        info!(%subject, "NATS worker subscribed");
        while let Some(msg) = sub.next().await {
            let topic = msg.subject.to_string();
            let payload = serde_json::from_slice::<Value>(&msg.payload).unwrap_or(Value::Null);
            handler(topic, payload).await?;
        }
        Ok(())
    }
}
