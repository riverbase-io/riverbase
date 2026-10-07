//! SSE stream of RxDB change notifications (`GET …/<collection>/stream`).

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use async_stream::stream;
use axum::response::sse::{Event, KeepAlive, Sse};
use chrono::Utc;
use futures_util::StreamExt;
use serde_json::{json, Value};

use crate::base::RiverbaseResult;
use crate::transport::StreamBus;

use super::service::rxdb_notify_channel;

/// Format a transport envelope for SSE clients (mirrors Python `stream_event`).
pub fn stream_event_data(envelope: Value) -> String {
    let mut event = match envelope {
        Value::Object(map) => map,
        other => {
            let mut map = serde_json::Map::new();
            map.insert("payload".to_string(), other);
            map
        }
    };
    event.insert("timestamp".to_string(), json!(Utc::now().to_rfc3339()));
    serde_json::to_string(&Value::Object(event)).unwrap_or_else(|_| "{}".to_string())
}

/// Subscribe to `rxdb.notify.<collection>` and stream Server-Sent Events.
pub async fn rxdb_sse_stream(
    bus: Arc<dyn StreamBus>,
    collection: String,
) -> RiverbaseResult<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>> + Send>> {
    let channel = rxdb_notify_channel(&collection)?;
    let mut values = bus.subscribe_values(&channel).await?;

    let body = stream! {
        yield Ok(Event::default().comment("keepalive"));
        let mut keepalive = tokio::time::interval(Duration::from_secs(30));
        keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = keepalive.tick() => {
                    yield Ok(Event::default().comment("keepalive"));
                }
                value = values.next() => {
                    match value {
                        Some(envelope) => {
                            if let Some(s) = envelope.as_str() {
                                if let Some(skipped) = s.strip_prefix("__lagged:") {
                                    yield Ok(Event::default().event("lagged").data(skipped));
                                    continue;
                                }
                            }
                            yield Ok(Event::default().data(stream_event_data(envelope)));
                        }
                        None => break,
                    }
                }
            }
        }
    };

    Ok(Sse::new(body).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keepalive"),
    ))
}
