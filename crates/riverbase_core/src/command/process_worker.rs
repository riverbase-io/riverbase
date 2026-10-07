use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use async_trait::async_trait;
use tracing::{error, warn};

use super::{ProcessManagerStore, ProcessState, ProcessStatus};
use crate::base::RiverbaseResult;

const CLAIM_LIMIT: i64 = 50;

/// Product hook that advances one leased workflow instance.
#[async_trait]
pub trait ProcessWorker: Send + Sync {
    /// Workflow type.
    fn workflow_type(&self) -> &str;
    /// Advance.
    async fn advance(&self, process: ProcessState) -> RiverbaseResult<ProcessState>;
}

/// Registry of workflow workers keyed by `workflow_type`.
#[derive(Default)]
pub struct ProcessManagerRegistry {
    workers: RwLock<HashMap<String, Arc<dyn ProcessWorker>>>,
}

impl ProcessManagerRegistry {
    /// Construct a new value.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Register.
    pub fn register(&self, worker: Arc<dyn ProcessWorker>) {
        self.workers
            .write()
            .expect("process manager registry lock")
            .insert(worker.workflow_type().to_string(), worker);
    }

    fn worker_for(&self, workflow_type: &str) -> Option<Arc<dyn ProcessWorker>> {
        self.workers
            .read()
            .expect("process manager registry lock")
            .get(workflow_type)
            .cloned()
    }
}

/// Claim and advance due workflow instances.
pub async fn deliver_process_manager_batch(
    store: &dyn ProcessManagerStore,
    registry: &ProcessManagerRegistry,
) -> RiverbaseResult<usize> {
    let due = store.claim_due(CLAIM_LIMIT).await?;
    let count = due.len();
    for mut process in due {
        let Some(worker) = registry.worker_for(&process.workflow_type) else {
            warn!(
                workflow_type = %process.workflow_type,
                process_id = %process.id,
                "no process worker registered; releasing lease"
            );
            process.status = ProcessStatus::Retrying;
            process.next_attempt_at = Some(chrono::Utc::now() + chrono::Duration::seconds(30));
            let version = process.version;
            store.save(None, process, version).await?;
            continue;
        };
        match worker.advance(process).await {
            Ok(_) => {}
            Err(err) => {
                warn!(
                    workflow_type = %worker.workflow_type(),
                    error = %err,
                    "process worker step failed"
                );
            }
        }
    }
    Ok(count)
}

/// Start the durable process-manager retry loop for one application runtime.
pub fn spawn_process_manager_worker(
    store: Arc<dyn ProcessManagerStore>,
    registry: Arc<ProcessManagerRegistry>,
) -> tokio::task::AbortHandle {
    let task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            if let Err(err) = deliver_process_manager_batch(store.as_ref(), registry.as_ref()).await
            {
                error!(error = %err, "process manager worker iteration failed");
            }
        }
    });
    task.abort_handle()
}
