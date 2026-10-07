use chrono::{DateTime, Utc};
use riverbase_core::base::TrackerId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Job lifecycle status (`riverbase.tracker.model.JobStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum JobStatus {
    Success,
    Error,
    Submitted,
    Received,
    #[serde(rename = "CANCELED")]
    Canceled,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "SUCCESS",
            Self::Error => "ERROR",
            Self::Submitted => "SUBMITTED",
            Self::Received => "RECEIVED",
            Self::Canceled => "CANCELED",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Success => "Finished",
            Self::Error => "Error",
            Self::Submitted => "Pending",
            Self::Canceled => "Canceled",
            Self::Received => "In Progress",
        }
    }

    pub fn from_db(value: &str) -> Self {
        match value {
            "SUCCESS" => Self::Success,
            "ERROR" => Self::Error,
            "SUBMITTED" => Self::Submitted,
            "RECEIVED" => Self::Received,
            "CANCELED" => Self::Canceled,
            _ => Self::Submitted,
        }
    }
}

/// Worker process status (`riverbase.tracker.model.WorkerStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorkerStatus {
    Started,
    Running,
    Stopped,
}

impl WorkerStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Started => "STARTED",
            Self::Running => "RUNNING",
            Self::Stopped => "STOPPED",
        }
    }

    pub fn from_db(value: &str) -> Self {
        match value {
            "STARTED" => Self::Started,
            "RUNNING" => Self::Running,
            "STOPPED" => Self::Stopped,
            _ => Self::Started,
        }
    }
}

/// Shared domain metadata on tracker rows (`riverbase_core::base::DomainFields`).
pub type TrackerRowMeta = riverbase_core::base::DomainFields;

pub fn new_tracker_meta(id: Option<TrackerId>) -> TrackerRowMeta {
    let mut meta = TrackerRowMeta::new();
    if let Some(id) = id {
        meta.id = id.0;
    }
    meta
}

/// `riverbase.tracker.model.Worker` (`worker` table).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worker {
    #[serde(flatten)]
    pub meta: TrackerRowMeta,
    pub pid: Option<i32>,
    pub status: Option<WorkerStatus>,
    pub hostname: Option<String>,
    pub queue_name: Option<String>,
    pub jobs_complete: Option<i32>,
    pub jobs_failed: Option<i32>,
    pub jobs_retried: Option<i32>,
    pub jobs_queued: Option<i32>,
    pub start_time: Option<DateTime<Utc>>,
    pub heart_beat: Option<DateTime<Utc>>,
    pub stop_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NewWorker {
    pub id: Option<TrackerId>,
    pub creator: Option<Uuid>,
    pub realm: Option<uuid::Uuid>,
    pub pid: Option<i32>,
    pub status: Option<WorkerStatus>,
    pub hostname: Option<String>,
    pub queue_name: Option<String>,
    pub jobs_complete: Option<i32>,
    pub jobs_failed: Option<i32>,
    pub jobs_retried: Option<i32>,
    pub jobs_queued: Option<i32>,
    pub start_time: Option<DateTime<Utc>>,
    pub heart_beat: Option<DateTime<Utc>>,
    pub stop_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkerUpdate {
    pub creator: Option<Uuid>,
    pub realm: Option<uuid::Uuid>,
    pub deleted: Option<DateTime<Utc>>,
    pub etag: Option<Uuid>,
    pub pid: Option<i32>,
    pub status: Option<WorkerStatus>,
    pub hostname: Option<String>,
    pub queue_name: Option<String>,
    pub jobs_complete: Option<i32>,
    pub jobs_failed: Option<i32>,
    pub jobs_retried: Option<i32>,
    pub jobs_queued: Option<i32>,
    pub start_time: Option<DateTime<Utc>>,
    pub heart_beat: Option<DateTime<Utc>>,
    pub stop_time: Option<DateTime<Utc>>,
}

impl NewWorker {
    pub fn into_worker(self) -> Worker {
        let mut meta = new_tracker_meta(self.id);
        meta.creator = self.creator;
        meta.tenant = self.realm;
        Worker {
            meta,
            pid: self.pid,
            status: self.status,
            hostname: self.hostname,
            queue_name: self.queue_name,
            jobs_complete: self.jobs_complete,
            jobs_failed: self.jobs_failed,
            jobs_retried: self.jobs_retried,
            jobs_queued: self.jobs_queued,
            start_time: self.start_time,
            heart_beat: self.heart_beat,
            stop_time: self.stop_time,
        }
    }
}

/// `riverbase.tracker.model.WorkerJob` (`worker_job` table).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerJob {
    #[serde(flatten)]
    pub meta: TrackerRowMeta,
    pub worker_id: Option<TrackerId>,
    pub job_message: Option<String>,
    pub job_progress: Option<f64>,
    pub job_status: Option<JobStatus>,
    pub job_try: Option<i32>,
    pub score: Option<i64>,
    pub queue_name: Option<String>,
    pub function: Option<String>,
    pub args: Option<Value>,
    pub kwargs: Option<Value>,
    pub result: Option<Value>,
    pub err_message: Option<String>,
    pub err_trace: Option<String>,
    pub enqueue_time: Option<DateTime<Utc>>,
    pub start_time: Option<DateTime<Utc>>,
    pub finish_time: Option<DateTime<Utc>>,
    pub defer_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NewWorkerJob {
    pub id: Option<TrackerId>,
    pub creator: Option<Uuid>,
    pub realm: Option<uuid::Uuid>,
    pub worker_id: Option<TrackerId>,
    pub job_message: Option<String>,
    pub job_progress: Option<f64>,
    pub job_status: Option<JobStatus>,
    pub job_try: Option<i32>,
    pub score: Option<i64>,
    pub queue_name: Option<String>,
    pub function: Option<String>,
    pub args: Option<Value>,
    pub kwargs: Option<Value>,
    pub result: Option<Value>,
    pub err_message: Option<String>,
    pub err_trace: Option<String>,
    pub enqueue_time: Option<DateTime<Utc>>,
    pub start_time: Option<DateTime<Utc>>,
    pub finish_time: Option<DateTime<Utc>>,
    pub defer_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkerJobUpdate {
    pub creator: Option<Uuid>,
    pub realm: Option<uuid::Uuid>,
    pub deleted: Option<DateTime<Utc>>,
    pub etag: Option<Uuid>,
    pub worker_id: Option<TrackerId>,
    pub job_message: Option<String>,
    pub job_progress: Option<f64>,
    pub job_status: Option<JobStatus>,
    pub job_try: Option<i32>,
    pub score: Option<i64>,
    pub queue_name: Option<String>,
    pub function: Option<String>,
    pub args: Option<Value>,
    pub kwargs: Option<Value>,
    pub result: Option<Value>,
    pub err_message: Option<String>,
    pub err_trace: Option<String>,
    pub enqueue_time: Option<DateTime<Utc>>,
    pub start_time: Option<DateTime<Utc>>,
    pub finish_time: Option<DateTime<Utc>>,
    pub defer_time: Option<DateTime<Utc>>,
}

impl NewWorkerJob {
    pub fn into_worker_job(self) -> WorkerJob {
        let mut meta = new_tracker_meta(self.id);
        meta.creator = self.creator;
        meta.tenant = self.realm;
        WorkerJob {
            meta,
            worker_id: self.worker_id,
            job_message: self.job_message,
            job_progress: self.job_progress,
            job_status: self.job_status.or(Some(JobStatus::Submitted)),
            job_try: self.job_try,
            score: self.score,
            queue_name: self.queue_name,
            function: self.function,
            args: self.args,
            kwargs: self.kwargs,
            result: self.result,
            err_message: self.err_message,
            err_trace: self.err_trace,
            enqueue_time: self.enqueue_time,
            start_time: self.start_time,
            finish_time: self.finish_time,
            defer_time: self.defer_time,
        }
    }
}

/// `riverbase.tracker.model.JobRelation` (`job_relation` table).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRelation {
    #[serde(flatten)]
    pub meta: TrackerRowMeta,
    pub job_id: Option<TrackerId>,
    pub resource: Option<String>,
    pub resource_id: Option<Uuid>,
    pub domain: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NewJobRelation {
    pub id: Option<TrackerId>,
    pub creator: Option<Uuid>,
    pub realm: Option<uuid::Uuid>,
    pub job_id: Option<TrackerId>,
    pub resource: Option<String>,
    pub resource_id: Option<Uuid>,
    pub domain: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JobRelationUpdate {
    pub creator: Option<Uuid>,
    pub realm: Option<uuid::Uuid>,
    pub deleted: Option<DateTime<Utc>>,
    pub etag: Option<Uuid>,
    pub job_id: Option<TrackerId>,
    pub resource: Option<String>,
    pub resource_id: Option<Uuid>,
    pub domain: Option<String>,
}

impl NewJobRelation {
    pub fn into_job_relation(self) -> JobRelation {
        let mut meta = new_tracker_meta(self.id);
        meta.creator = self.creator;
        meta.tenant = self.realm;
        JobRelation {
            meta,
            job_id: self.job_id,
            resource: self.resource,
            resource_id: self.resource_id,
            domain: self.domain,
        }
    }
}
