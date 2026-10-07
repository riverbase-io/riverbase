use crate::tracker::model::{
    JobRelation, JobRelationUpdate, Worker, WorkerJob, WorkerJobUpdate, WorkerUpdate,
};

pub(crate) fn apply_worker_update(worker: &mut Worker, patch: WorkerUpdate) {
    if let Some(v) = patch.creator {
        worker.meta.creator = Some(v);
    }
    if let Some(v) = patch.realm {
        worker.meta.tenant = Some(v);
    }
    if let Some(v) = patch.deleted {
        worker.meta.deleted = Some(v);
    }
    if let Some(v) = patch.etag {
        worker.meta.etag = Some(v);
    }
    if let Some(v) = patch.pid {
        worker.pid = Some(v);
    }
    if let Some(v) = patch.status {
        worker.status = Some(v);
    }
    if let Some(v) = patch.hostname {
        worker.hostname = Some(v);
    }
    if let Some(v) = patch.queue_name {
        worker.queue_name = Some(v);
    }
    if let Some(v) = patch.jobs_complete {
        worker.jobs_complete = Some(v);
    }
    if let Some(v) = patch.jobs_failed {
        worker.jobs_failed = Some(v);
    }
    if let Some(v) = patch.jobs_retried {
        worker.jobs_retried = Some(v);
    }
    if let Some(v) = patch.jobs_queued {
        worker.jobs_queued = Some(v);
    }
    if let Some(v) = patch.start_time {
        worker.start_time = Some(v);
    }
    if let Some(v) = patch.heart_beat {
        worker.heart_beat = Some(v);
    }
    if let Some(v) = patch.stop_time {
        worker.stop_time = Some(v);
    }
}

pub(crate) fn apply_worker_job_update(job: &mut WorkerJob, patch: WorkerJobUpdate) {
    if let Some(v) = patch.creator {
        job.meta.creator = Some(v);
    }
    if let Some(v) = patch.realm {
        job.meta.tenant = Some(v);
    }
    if let Some(v) = patch.deleted {
        job.meta.deleted = Some(v);
    }
    if let Some(v) = patch.etag {
        job.meta.etag = Some(v);
    }
    if let Some(v) = patch.worker_id {
        job.worker_id = Some(v);
    }
    if let Some(v) = patch.job_message {
        job.job_message = Some(v);
    }
    if let Some(v) = patch.job_progress {
        job.job_progress = Some(v);
    }
    if let Some(v) = patch.job_status {
        job.job_status = Some(v);
    }
    if let Some(v) = patch.job_try {
        job.job_try = Some(v);
    }
    if let Some(v) = patch.score {
        job.score = Some(v);
    }
    if let Some(v) = patch.queue_name {
        job.queue_name = Some(v);
    }
    if let Some(v) = patch.function {
        job.function = Some(v);
    }
    if let Some(v) = patch.args {
        job.args = Some(v);
    }
    if let Some(v) = patch.kwargs {
        job.kwargs = Some(v);
    }
    if let Some(v) = patch.result {
        job.result = Some(v);
    }
    if let Some(v) = patch.err_message {
        job.err_message = Some(v);
    }
    if let Some(v) = patch.err_trace {
        job.err_trace = Some(v);
    }
    if let Some(v) = patch.enqueue_time {
        job.enqueue_time = Some(v);
    }
    if let Some(v) = patch.start_time {
        job.start_time = Some(v);
    }
    if let Some(v) = patch.finish_time {
        job.finish_time = Some(v);
    }
    if let Some(v) = patch.defer_time {
        job.defer_time = Some(v);
    }
}

pub(crate) fn apply_job_relation_update(relation: &mut JobRelation, patch: JobRelationUpdate) {
    if let Some(v) = patch.creator {
        relation.meta.creator = Some(v);
    }
    if let Some(v) = patch.realm {
        relation.meta.tenant = Some(v);
    }
    if let Some(v) = patch.deleted {
        relation.meta.deleted = Some(v);
    }
    if let Some(v) = patch.etag {
        relation.meta.etag = Some(v);
    }
    if let Some(v) = patch.job_id {
        relation.job_id = Some(v);
    }
    if let Some(v) = patch.resource {
        relation.resource = Some(v);
    }
    if let Some(v) = patch.resource_id {
        relation.resource_id = Some(v);
    }
    if let Some(v) = patch.domain {
        relation.domain = Some(v);
    }
}
