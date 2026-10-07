//! Worker runtime and job tracker for Riverbase.

pub mod errors;
pub mod tracker;
pub mod worker;

#[cfg(feature = "postgres")]
pub mod postgres;

pub use tracker::{
    JobRelation, JobRelationUpdate, JobStatus, NewJobRelation, NewWorker, NewWorkerJob,
    TrackerRowMeta, TrackerStore, Worker, WorkerJob, WorkerJobUpdate, WorkerStatus, WorkerUpdate,
};
#[cfg(feature = "nats-io")]
pub use worker::NatsMessageConsumer;
pub use worker::{MessageConsumer, WorkerRuntime};

#[cfg(feature = "postgres")]
pub use postgres::{
    establish_tracker_dbpool, run_tracker_migrations, run_tracker_migrations_url,
    PostgresTrackerStore,
};
