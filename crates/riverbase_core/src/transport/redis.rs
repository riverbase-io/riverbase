use crate::base::RiverbaseResult;
use crate::command::MessageBus;
use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::Mutex;

/// Redis Pub/Sub-backed message bus.
pub struct RedisMessageBus {
    client: ::redis::Client,
    connection: Mutex<::redis::aio::MultiplexedConnection>,
}

impl RedisMessageBus {
    /// Connect.
    pub async fn connect(redis_url: &str) -> RiverbaseResult<Self> {
        let client = ::redis::Client::open(redis_url)
            .map_err(|e| crate::errors::TRN_004.with_data(e.to_string()))?;
        let connection = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| crate::errors::TRN_005.with_data(e.to_string()))?;
        Ok(Self {
            client,
            connection: Mutex::new(connection),
        })
    }

    /// Subscribe to a Redis pub/sub channel.
    pub async fn subscribe_pubsub(&self, channel: &str) -> RiverbaseResult<::redis::aio::PubSub> {
        let mut pubsub = self
            .client
            .get_async_pubsub()
            .await
            .map_err(|e| crate::errors::TRN_010.with_data(e.to_string()))?;
        pubsub
            .subscribe(channel)
            .await
            .map_err(|e| crate::errors::TRN_011.with_data(e.to_string()))?;
        Ok(pubsub)
    }
}

#[async_trait]
impl MessageBus for RedisMessageBus {
    async fn publish(&self, topic: &str, payload: Value) -> RiverbaseResult<()> {
        use ::redis::AsyncCommands;

        let message = serde_json::to_string(&payload)
            .map_err(|e| crate::errors::TRN_006.with_data(e.to_string()))?;
        let mut connection = self.connection.lock().await;
        let _: usize = connection
            .publish(topic, message)
            .await
            .map_err(|e| crate::errors::TRN_007.with_data(e.to_string()))?;
        Ok(())
    }
}
