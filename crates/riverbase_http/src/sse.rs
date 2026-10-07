//! Server-Sent Event adapters for [`StreamBus`].

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use async_stream::stream;
use axum::response::sse::{Event, KeepAlive, Sse};
use chrono::Utc;
use riverbase_core::base::RiverbaseResult;
use riverbase_core::transport::StreamBus;
use futures_util::StreamExt;
use serde_json::{json, Value};

fn sse_event_data(envelope: Value) -> String {
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

/// SSE stream from any [`StreamBus`] implementation ([RUN-02]).
pub async fn bus_sse_stream(
    bus: Arc<dyn StreamBus>,
    topic: String,
) -> RiverbaseResult<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>> + Send>> {
    let mut values = bus.subscribe_values(&topic).await?;

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
                            yield Ok(Event::default().data(sse_event_data(envelope)));
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
