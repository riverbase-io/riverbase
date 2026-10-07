use std::sync::Arc;
use std::time::Duration;

use tracing::{error, warn};

use super::MessageBus;
use crate::base::RiverbaseResult;
use crate::logstore::OutboxStore;

const CLAIM_LIMIT: i64 = 100;
const LEASE: Duration = Duration::from_secs(30);
const MAX_ATTEMPTS: i32 = 10;
const MAX_BACKOFF_SECONDS: u64 = 300;

/// Deliver one leased outbox batch.
pub async fn deliver_outbox_batch(
    outbox: &dyn OutboxStore,
    msgbus: &dyn MessageBus,
) -> RiverbaseResult<usize> {
    let records = outbox.claim_due(CLAIM_LIMIT, LEASE).await?;
    let count = records.len();
    for record in records {
        match msgbus.publish(&record.topic, record.payload.clone()).await {
            Ok(()) => outbox.mark_published(record.id).await?,
            Err(err) => {
                let exponent = record.attempts.clamp(1, 16) as u32;
                let seconds = 2_u64.saturating_pow(exponent).min(MAX_BACKOFF_SECONDS);
                warn!(
                    outbox_id = %record.id,
                    topic = %record.topic,
                    attempts = record.attempts,
                    retry_seconds = seconds,
                    error = %err,
                    "outbox delivery failed"
                );
                outbox
                    .mark_failed(
                        record.id,
                        &err.to_string(),
                        Duration::from_secs(seconds),
                        MAX_ATTEMPTS,
                    )
                    .await?;
            }
        }
    }
    Ok(count)
}

/// Start the durable outbox retry loop for one application runtime.
pub fn spawn_outbox_publisher(
    outbox: Arc<dyn OutboxStore>,
    msgbus: Arc<dyn MessageBus>,
) -> tokio::task::AbortHandle {
    let task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            if let Err(err) = deliver_outbox_batch(outbox.as_ref(), msgbus.as_ref()).await {
                error!(error = %err, "outbox publisher iteration failed");
            }
        }
    });
    task.abort_handle()
}
