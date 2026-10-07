use async_trait::async_trait;

use super::model::{
    JobRelation, JobRelationUpdate, NewJobRelation, NewWorker, NewWorkerJob, Worker, WorkerJob,
    WorkerJobUpdate, WorkerUpdate,
};
use riverbase_core::base::{RiverbaseResult, TrackerId};
use riverbase_core::logstore::{ActivityLogRecord, LogStore};

/// Persistence port aligned with Python `SQLTrackerManager`.
#[async_trait]
pub trait TrackerStore: Send + Sync {
    async fn add_worker(&self, row: NewWorker) -> RiverbaseResult<Worker>;
    async fn update_worker(&self, id: &TrackerId, patch: WorkerUpdate) -> RiverbaseResult<Worker>;
    async fn fetch_worker(&self, id: &TrackerId) -> RiverbaseResult<Worker>;

    async fn add_worker_job(&self, row: NewWorkerJob) -> RiverbaseResult<WorkerJob>;
    async fn update_worker_job(
        &self,
        id: &TrackerId,
        patch: WorkerJobUpdate,
        activities: Option<&dyn LogStore<ActivityLogRecord>>,
    ) -> RiverbaseResult<WorkerJob>;
    async fn fetch_worker_job(&self, id: &TrackerId) -> RiverbaseResult<WorkerJob>;

    async fn add_job_relation(&self, row: NewJobRelation) -> RiverbaseResult<JobRelation>;
    async fn update_job_relation(
        &self,
        id: &TrackerId,
        patch: JobRelationUpdate,
    ) -> RiverbaseResult<JobRelation>;
    async fn fetch_job_relation(&self, id: &TrackerId) -> RiverbaseResult<JobRelation>;
}
