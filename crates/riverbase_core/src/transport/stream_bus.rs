//! Pluggable realtime bus with publish + subscribe ([RUN-01], [RUN-02]).

use std::pin::Pin;
use std::sync::Arc;

use async_stream::stream;
use async_trait::async_trait;
use futures_util::Stream;
use futures_util::StreamExt;
use serde_json::Value;
use tokio_postgres::AsyncMessage;

use crate::base::RiverbaseResult;
use crate::command::MessageBus;

use super::postgres::{connect_pair, pg_channel, pg_error_detail, PgMessageBus};

/// Cross-replica fanout bus: command outbox publish + SSE/RTC subscribe.
#[async_trait]
pub trait StreamBus: MessageBus {
    /// Subscribe to a topic; yields JSON payloads until the stream ends.
    async fn subscribe_values(
        &self,
        topic: &str,
    ) -> RiverbaseResult<Pin<Box<dyn Stream<Item = Value> + Send>>>;
}

#[async_trait]
impl StreamBus for PgMessageBus {
    async fn subscribe_values(
        &self,
        topic: &str,
    ) -> RiverbaseResult<Pin<Box<dyn Stream<Item = Value> + Send>>> {
        let channel = pg_channel(topic);
        let bus = Arc::new(PgMessageBus::connect(self.dsn()).await?);
        let (client, mut connection) = connect_pair(bus.dsn()).await?;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        tokio::spawn(async move {
            let mut stream = futures_util::stream::poll_fn(move |cx| {
                connection.poll_message(cx).map(|ready| match ready {
                    Some(Ok(msg)) => Some(Ok(msg)),
                    Some(Err(e)) => Some(Err(e)),
                    None => None,
                })
            });
            while let Some(item) = stream.next().await {
                match item {
                    Ok(AsyncMessage::Notification(n)) => {
                        if tx.send(n.payload().to_string()).is_err() {
                            break;
                        }
                    }
                    Ok(AsyncMessage::Notice(_)) => {}
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        });
        let listen_sql = format!("LISTEN {}", quote_ident(&channel));
        client
            .batch_execute(&listen_sql)
            .await
            .map_err(|e| crate::errors::TRN_030.with_data(pg_error_detail(&e)))?;
        // Keep client alive for the duration of the listen.
        let _keepalive = client;
        Ok(Box::pin(stream! {
            while let Some(raw) = rx.recv().await {
                match bus.resolve_notify_payload(&raw).await {
                    Ok(value) => yield value,
                    Err(err) => {
                        tracing::warn!(error = %err, "postgres bus notify decode failed");
                    }
                }
            }
        }))
    }
}

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

#[cfg(feature = "nats-io")]
#[async_trait]
impl StreamBus for super::nats::NatsMessageBus {
    async fn subscribe_values(
        &self,
        topic: &str,
    ) -> RiverbaseResult<Pin<Box<dyn Stream<Item = Value> + Send>>> {
        let mut sub = self.subscribe(topic).await?;
        Ok(Box::pin(stream! {
            while let Some(frame) = sub.next().await {
                if let Ok(value) = serde_json::from_slice::<Value>(&frame.payload) {
                    yield value;
                }
            }
        }))
    }
}

#[cfg(feature = "redis")]
#[async_trait]
impl StreamBus for super::redis::RedisMessageBus {
    async fn subscribe_values(
        &self,
        topic: &str,
    ) -> RiverbaseResult<Pin<Box<dyn Stream<Item = Value> + Send>>> {
        let pubsub = self.subscribe_pubsub(topic).await?;
        let mut messages = pubsub.into_on_message();
        Ok(Box::pin(stream! {
            while let Some(msg) = messages.next().await {
                let Ok(payload) = msg.get_payload::<String>() else {
                    continue;
                };
                if let Ok(value) = serde_json::from_str::<Value>(&payload) {
                    yield value;
                }
            }
        }))
    }
}
