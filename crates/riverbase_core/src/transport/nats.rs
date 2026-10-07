use crate::base::RiverbaseResult;
use crate::command::MessageBus;
use async_trait::async_trait;
use serde_json::Value;

/// NATS-backed message bus (publish + subscribe for workers).
#[derive(Clone)]
pub struct NatsMessageBus {
    client: async_nats::Client,
}

impl NatsMessageBus {
    /// Connect.
    pub async fn connect(server_addr: &str) -> RiverbaseResult<Self> {
        let client = async_nats::connect(server_addr)
            .await
            .map_err(|e| crate::errors::TRN_001.with_data(e.to_string()))?;
        Ok(Self { client })
    }

    /// Subscribe to a subject or wildcard (e.g. `flrs.todo.>`).
    pub async fn subscribe(
        &self,
        subject: impl Into<String>,
    ) -> RiverbaseResult<async_nats::Subscriber> {
        self.client
            .subscribe(subject.into())
            .await
            .map_err(|e| crate::errors::TRN_031.with_data(e.to_string()))
    }
}

#[async_trait]
impl MessageBus for NatsMessageBus {
    async fn publish(&self, topic: &str, payload: Value) -> RiverbaseResult<()> {
        let bytes = serde_json::to_vec(&payload)
            .map_err(|e| crate::errors::TRN_002.with_data(e.to_string()))?;
        self.client
            .publish(topic.to_string(), bytes.into())
            .await
            .map_err(|e| crate::errors::TRN_003.with_data(e.to_string()))?;
        Ok(())
    }
}
