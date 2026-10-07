use std::sync::Arc;

use crate::tracker::{JobStatus, NewWorkerJob, TrackerStore, WorkerJobUpdate};
use chrono::Utc;
use riverbase_core::base::{RiverbaseResult, TrackerId};
use riverbase_core::domain::DomainCommandEngine;
use riverbase_core::transport::StreamBus;
#[cfg(feature = "nats-io")]
use tracing::info;

use super::consumer::MessageConsumer;
#[cfg(feature = "nats-io")]
use super::consumer::NatsMessageConsumer;

pub struct WorkerRuntime {
    bus: Option<Arc<dyn StreamBus>>,
    pub tracker: Arc<dyn TrackerStore>,
}

impl WorkerRuntime {
    pub fn new(bus: Arc<dyn StreamBus>, tracker: Arc<dyn TrackerStore>) -> Self {
        Self {
            bus: Some(bus),
            tracker,
        }
    }

    pub fn with_tracker(tracker: Arc<dyn TrackerStore>) -> Self {
        Self { bus: None, tracker }
    }

    pub async fn run_command_side_effect(
        &self,
        _command: &dyn DomainCommandEngine,
        activities: Option<
            &dyn riverbase_core::logstore::LogStore<riverbase_core::logstore::ActivityLogRecord>,
        >,
    ) -> RiverbaseResult<()> {
        let bus = self
            .bus
            .as_ref()
            .ok_or_else(|| crate::errors::WRK_001.with_data("bus not configured"))?;
        let consumer = MessageConsumer::new(bus.clone(), "riverbase.worker.side_effect");
        let job = self
            .tracker
            .add_worker_job(NewWorkerJob {
                function: Some("todo-worker".into()),
                queue_name: Some("default".into()),
                ..Default::default()
            })
            .await?;
        self.tracker
            .update_worker_job(
                &TrackerId(job.meta.id),
                WorkerJobUpdate {
                    job_status: Some(JobStatus::Received),
                    start_time: Some(Utc::now()),
                    ..Default::default()
                },
                activities,
            )
            .await?;
        let _ = consumer.drain_once().await;
        self.tracker
            .update_worker_job(
                &TrackerId(job.meta.id),
                WorkerJobUpdate {
                    job_status: Some(JobStatus::Success),
                    finish_time: Some(Utc::now()),
                    job_progress: Some(100.0),
                    ..Default::default()
                },
                activities,
            )
            .await?;
        Ok(())
    }

    /// Subscribe on NATS and record a tracker job for each consumed message.
    #[cfg(feature = "nats-io")]
    pub async fn run_nats_consumer(
        &self,
        bus: &riverbase_core::transport::NatsMessageBus,
        subject: &str,
        queue_name: &str,
        activities: Option<
            &dyn riverbase_core::logstore::LogStore<riverbase_core::logstore::ActivityLogRecord>,
        >,
    ) -> RiverbaseResult<()> {
        let consumer = NatsMessageConsumer::new(bus.clone());
        consumer
            .run(subject, |topic, payload| {
                let tracker = self.tracker.clone();
                async move {
                    let job = tracker
                        .add_worker_job(NewWorkerJob {
                            function: Some(format!("nats:{topic}")),
                            queue_name: Some(queue_name.into()),
                            ..Default::default()
                        })
                        .await?;
                    tracker
                        .update_worker_job(
                            &TrackerId(job.meta.id),
                            WorkerJobUpdate {
                                job_status: Some(JobStatus::Received),
                                start_time: Some(Utc::now()),
                                ..Default::default()
                            },
                            activities,
                        )
                        .await?;
                    info!(%topic, ?payload, job_id = %job.meta.id, "NATS worker consumed message");
                    tracker
                        .update_worker_job(
                            &TrackerId(job.meta.id),
                            WorkerJobUpdate {
                                job_status: Some(JobStatus::Success),
                                finish_time: Some(Utc::now()),
                                job_progress: Some(100.0),
                                ..Default::default()
                            },
                            activities,
                        )
                        .await?;
                    Ok(())
                }
            })
            .await
    }
}
