//! Job tracker models aligned with `riverbase.tracker.model`.

pub mod config;
pub mod model;
pub(crate) mod patch;
pub mod store;

#[cfg(feature = "postgres")]
pub use crate::postgres::PostgresTrackerStore;
pub use config::{
    COLLECT_TRACEBACK, JOB_RELATION_TABLE, TRACKER_DATA_SCHEMA, WORKER_JOB_TABLE, WORKER_TABLE,
};
pub use model::{
    JobRelation, JobRelationUpdate, JobStatus, NewJobRelation, NewWorker, NewWorkerJob,
    TrackerRowMeta, Worker, WorkerJob, WorkerJobUpdate, WorkerStatus, WorkerUpdate,
};
pub use store::TrackerStore;
