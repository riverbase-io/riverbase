use async_trait::async_trait;
use chrono::Utc;
use diesel::prelude::*;
use riverbase_core::datastore::postgres::exec;
use riverbase_core::datastore::postgres::SyncPgPool;

use super::schema::{job_relation, worker, worker_job};
use crate::tracker::model::{
    JobRelation, JobRelationUpdate, JobStatus, NewJobRelation, NewWorker, NewWorkerJob,
    TrackerRowMeta, Worker, WorkerJob, WorkerJobUpdate, WorkerStatus, WorkerUpdate,
};
use crate::tracker::store::TrackerStore;
use riverbase_core::base::{RiverbaseResult, TrackerId};
use riverbase_core::logstore::{
    log_uuid_from_command_id, new_log_id, ActivityLogRecord, ActivityMsgType, LogRowMeta, LogStore,
};

pub struct PostgresTrackerStore {
    dbpool: SyncPgPool,
}

impl PostgresTrackerStore {
    pub fn new(dbpool: SyncPgPool) -> Self {
        Self { dbpool }
    }
}

#[async_trait]
impl TrackerStore for PostgresTrackerStore {
    async fn add_worker(&self, row: NewWorker) -> RiverbaseResult<Worker> {
        let worker = row.into_worker();
        let worker_clone = worker.clone();
        exec::run_blocking_riverbase(&self.dbpool, move |conn| insert_worker(conn, &worker_clone))
            .await?;
        Ok(worker)
    }

    async fn update_worker(&self, id: &TrackerId, patch: WorkerUpdate) -> RiverbaseResult<Worker> {
        let mut worker = self.fetch_worker(id).await?;
        apply_worker_update(&mut worker, patch);
        let worker_clone = worker.clone();
        exec::run_blocking_riverbase(&self.dbpool, move |conn| {
            update_worker_row(conn, &worker_clone)
        })
        .await?;
        Ok(worker)
    }

    async fn fetch_worker(&self, id: &TrackerId) -> RiverbaseResult<Worker> {
        let id_val = id.0;
        exec::run_blocking_riverbase(&self.dbpool, move |conn| fetch_worker_row(conn, id_val)).await
    }

    async fn add_worker_job(&self, row: NewWorkerJob) -> RiverbaseResult<WorkerJob> {
        let job = row.into_worker_job();
        let job_clone = job.clone();
        exec::run_blocking_riverbase(&self.dbpool, move |conn| {
            insert_worker_job(conn, &job_clone)
        })
        .await?;
        Ok(job)
    }

    async fn update_worker_job(
        &self,
        id: &TrackerId,
        patch: WorkerJobUpdate,
        activities: Option<&dyn LogStore<ActivityLogRecord>>,
    ) -> RiverbaseResult<WorkerJob> {
        let status_changed = patch.job_status.is_some();
        let mut job = self.fetch_worker_job(id).await?;
        apply_worker_job_update(&mut job, patch);
        let job_clone = job.clone();
        exec::run_blocking_riverbase(&self.dbpool, move |conn| {
            update_worker_job_row(conn, &job_clone)
        })
        .await?;

        if status_changed {
            if let Some(store) = activities {
                let cmd_id = riverbase_core::base::CommandId::new();
                store
                    .append(
                        None,
                        ActivityLogRecord {
                            meta: LogRowMeta {
                                id: new_log_id(),
                                created: Utc::now(),
                                creator: None,
                            },
                            source: Some("flrs-tracker".into()),
                            domain: "riverbase.tracker".into(),
                            identifier: Some(job.meta.id),
                            resource: "worker_job".into(),
                            domain_sid: None,
                            domain_iid: None,
                            message: "Job status changed".into(),
                            msgtype: ActivityMsgType::SystemCall,
                            msglabel: "tracker.job_status_changed".into(),
                            context: None,
                            src_cmd: Some(log_uuid_from_command_id(&cmd_id)),
                            src_evt: None,
                            data: Some(serde_json::json!({
                                "job_id": job.meta.id.to_string(),
                                "job_status": job.job_status.map(|s| s.as_str()),
                            })),
                            code: 0,
                            tenant: None,
                        },
                    )
                    .await?;
            }
        }
        Ok(job)
    }

    async fn fetch_worker_job(&self, id: &TrackerId) -> RiverbaseResult<WorkerJob> {
        let id_val = id.0;
        exec::run_blocking_riverbase(&self.dbpool, move |conn| fetch_worker_job_row(conn, id_val))
            .await
    }

    async fn add_job_relation(&self, row: NewJobRelation) -> RiverbaseResult<JobRelation> {
        let relation = row.into_job_relation();
        let relation_clone = relation.clone();
        exec::run_blocking_riverbase(&self.dbpool, move |conn| {
            insert_job_relation(conn, &relation_clone)
        })
        .await?;
        Ok(relation)
    }

    async fn update_job_relation(
        &self,
        id: &TrackerId,
        patch: JobRelationUpdate,
    ) -> RiverbaseResult<JobRelation> {
        let mut relation = self.fetch_job_relation(id).await?;
        apply_job_relation_update(&mut relation, patch);
        let relation_clone = relation.clone();
        exec::run_blocking_riverbase(&self.dbpool, move |conn| {
            update_job_relation_row(conn, &relation_clone)
        })
        .await?;
        Ok(relation)
    }

    async fn fetch_job_relation(&self, id: &TrackerId) -> RiverbaseResult<JobRelation> {
        let id_val = id.0;
        exec::run_blocking_riverbase(&self.dbpool, move |conn| {
            fetch_job_relation_row(conn, id_val)
        })
        .await
    }
}

fn insert_worker(conn: &mut diesel::pg::PgConnection, worker: &Worker) -> RiverbaseResult<()> {
    diesel::insert_into(worker::table)
        .values((
            worker::_id.eq(worker.meta.id),
            worker::_created.eq(worker.meta.created),
            worker::_updated.eq(worker.meta.updated),
            worker::_creator.eq(worker.meta.creator),
            worker::_updater.eq(worker.meta.updater),
            worker::_tenant.eq(worker.meta.tenant),
            worker::_etag.eq(worker.meta.etag.unwrap_or_else(uuid::Uuid::new_v4)),
            worker::_deleted.eq(worker.meta.deleted),
            worker::pid.eq(worker.pid),
            worker::status.eq(worker.status.map(|s| s.as_str().to_string())),
            worker::hostname.eq(worker.hostname.as_deref()),
            worker::queue_name.eq(worker.queue_name.as_deref()),
            worker::jobs_complete.eq(worker.jobs_complete),
            worker::jobs_failed.eq(worker.jobs_failed),
            worker::jobs_retried.eq(worker.jobs_retried),
            worker::jobs_queued.eq(worker.jobs_queued),
            worker::start_time.eq(worker.start_time),
            worker::heart_beat.eq(worker.heart_beat),
            worker::stop_time.eq(worker.stop_time),
        ))
        .execute(conn)
        .map_err(|e| crate::errors::TRK_017.with_data(e.to_string()))?;
    Ok(())
}

fn update_worker_row(conn: &mut diesel::pg::PgConnection, worker: &Worker) -> RiverbaseResult<()> {
    diesel::update(worker::table.filter(worker::_id.eq(worker.meta.id)))
        .set((
            worker::_updated.eq(worker.meta.updated),
            worker::_creator.eq(worker.meta.creator),
            worker::_updater.eq(worker.meta.updater),
            worker::_tenant.eq(worker.meta.tenant),
            worker::_etag.eq(worker.meta.etag.unwrap_or_else(uuid::Uuid::new_v4)),
            worker::_deleted.eq(worker.meta.deleted),
            worker::pid.eq(worker.pid),
            worker::status.eq(worker.status.map(|s| s.as_str().to_string())),
            worker::hostname.eq(worker.hostname.as_deref()),
            worker::queue_name.eq(worker.queue_name.as_deref()),
            worker::jobs_complete.eq(worker.jobs_complete),
            worker::jobs_failed.eq(worker.jobs_failed),
            worker::jobs_retried.eq(worker.jobs_retried),
            worker::jobs_queued.eq(worker.jobs_queued),
            worker::start_time.eq(worker.start_time),
            worker::heart_beat.eq(worker.heart_beat),
            worker::stop_time.eq(worker.stop_time),
        ))
        .execute(conn)
        .map_err(|e| crate::errors::TRK_019.with_data(e.to_string()))?;
    Ok(())
}

fn fetch_worker_row(conn: &mut diesel::pg::PgConnection, id: uuid::Uuid) -> RiverbaseResult<Worker> {
    #[derive(Queryable)]
    struct Row {
        _id: uuid::Uuid,
        _created: chrono::DateTime<Utc>,
        _updated: Option<chrono::DateTime<Utc>>,
        _creator: Option<uuid::Uuid>,
        _updater: Option<uuid::Uuid>,
        _tenant: Option<uuid::Uuid>,
        _etag: uuid::Uuid,
        _deleted: Option<chrono::DateTime<Utc>>,
        pid: Option<i32>,
        status: Option<String>,
        hostname: Option<String>,
        queue_name: Option<String>,
        jobs_complete: Option<i32>,
        jobs_failed: Option<i32>,
        jobs_retried: Option<i32>,
        jobs_queued: Option<i32>,
        start_time: Option<chrono::DateTime<Utc>>,
        heart_beat: Option<chrono::DateTime<Utc>>,
        stop_time: Option<chrono::DateTime<Utc>>,
    }
    let row: Row = worker::table
        .filter(worker::_id.eq(id))
        .first(conn)
        .map_err(|_| crate::errors::TRK_021.with_data(id.to_string()))?;
    Ok(Worker {
        meta: TrackerRowMeta {
            id: row._id,
            created: row._created,
            updated: row._updated,
            creator: row._creator,
            updater: row._updater,
            tenant: row._tenant,
            etag: Some(row._etag),
            deleted: row._deleted,
        },
        pid: row.pid,
        status: row.status.map(|s| WorkerStatus::from_db(&s)),
        hostname: row.hostname,
        queue_name: row.queue_name,
        jobs_complete: row.jobs_complete,
        jobs_failed: row.jobs_failed,
        jobs_retried: row.jobs_retried,
        jobs_queued: row.jobs_queued,
        start_time: row.start_time,
        heart_beat: row.heart_beat,
        stop_time: row.stop_time,
    })
}

fn insert_worker_job(conn: &mut diesel::pg::PgConnection, job: &WorkerJob) -> RiverbaseResult<()> {
    diesel::insert_into(worker_job::table)
        .values((
            worker_job::_id.eq(job.meta.id),
            worker_job::_created.eq(job.meta.created),
            worker_job::_updated.eq(job.meta.updated),
            worker_job::_creator.eq(job.meta.creator),
            worker_job::_updater.eq(job.meta.updater),
            worker_job::_tenant.eq(job.meta.tenant),
            worker_job::_etag.eq(job.meta.etag.unwrap_or_else(uuid::Uuid::new_v4)),
            worker_job::_deleted.eq(job.meta.deleted),
            worker_job::worker_id.eq(job.worker_id.as_ref().map(|id| id.0)),
            worker_job::job_message.eq(job.job_message.as_deref()),
            worker_job::job_progress.eq(job.job_progress),
            worker_job::job_status.eq(job.job_status.map(|s| s.as_str().to_string())),
            worker_job::job_try.eq(job.job_try),
            worker_job::score.eq(job.score),
            worker_job::queue_name.eq(job.queue_name.as_deref()),
            worker_job::function.eq(job.function.as_deref()),
            worker_job::args.eq(job.args.clone()),
            worker_job::kwargs.eq(job.kwargs.clone()),
            worker_job::result.eq(job.result.clone()),
            worker_job::err_message.eq(job.err_message.as_deref()),
            worker_job::err_trace.eq(job.err_trace.as_deref()),
            worker_job::enqueue_time.eq(job.enqueue_time),
            worker_job::start_time.eq(job.start_time),
            worker_job::finish_time.eq(job.finish_time),
            worker_job::defer_time.eq(job.defer_time),
        ))
        .execute(conn)
        .map_err(|e| crate::errors::TRK_023.with_data(e.to_string()))?;
    Ok(())
}

fn update_worker_job_row(
    conn: &mut diesel::pg::PgConnection,
    job: &WorkerJob,
) -> RiverbaseResult<()> {
    diesel::update(worker_job::table.filter(worker_job::_id.eq(job.meta.id)))
        .set((
            worker_job::_updated.eq(job.meta.updated),
            worker_job::_creator.eq(job.meta.creator),
            worker_job::_updater.eq(job.meta.updater),
            worker_job::_tenant.eq(job.meta.tenant),
            worker_job::_etag.eq(job.meta.etag.unwrap_or_else(uuid::Uuid::new_v4)),
            worker_job::_deleted.eq(job.meta.deleted),
            worker_job::worker_id.eq(job.worker_id.as_ref().map(|id| id.0)),
            worker_job::job_message.eq(job.job_message.as_deref()),
            worker_job::job_progress.eq(job.job_progress),
            worker_job::job_status.eq(job.job_status.map(|s| s.as_str().to_string())),
            worker_job::job_try.eq(job.job_try),
            worker_job::score.eq(job.score),
            worker_job::queue_name.eq(job.queue_name.as_deref()),
            worker_job::function.eq(job.function.as_deref()),
            worker_job::args.eq(job.args.clone()),
            worker_job::kwargs.eq(job.kwargs.clone()),
            worker_job::result.eq(job.result.clone()),
            worker_job::err_message.eq(job.err_message.as_deref()),
            worker_job::err_trace.eq(job.err_trace.as_deref()),
            worker_job::enqueue_time.eq(job.enqueue_time),
            worker_job::start_time.eq(job.start_time),
            worker_job::finish_time.eq(job.finish_time),
            worker_job::defer_time.eq(job.defer_time),
        ))
        .execute(conn)
        .map_err(|e| crate::errors::TRK_025.with_data(e.to_string()))?;
    Ok(())
}

fn fetch_worker_job_row(
    conn: &mut diesel::pg::PgConnection,
    id: uuid::Uuid,
) -> RiverbaseResult<WorkerJob> {
    #[derive(Queryable)]
    struct Row {
        _id: uuid::Uuid,
        _created: chrono::DateTime<Utc>,
        _updated: Option<chrono::DateTime<Utc>>,
        _creator: Option<uuid::Uuid>,
        _updater: Option<uuid::Uuid>,
        _tenant: Option<uuid::Uuid>,
        _etag: uuid::Uuid,
        _deleted: Option<chrono::DateTime<Utc>>,
        worker_id: Option<uuid::Uuid>,
        job_message: Option<String>,
        job_progress: Option<f64>,
        job_status: Option<String>,
        job_try: Option<i32>,
        score: Option<i64>,
        queue_name: Option<String>,
        function: Option<String>,
        args: Option<serde_json::Value>,
        kwargs: Option<serde_json::Value>,
        result: Option<serde_json::Value>,
        err_message: Option<String>,
        err_trace: Option<String>,
        enqueue_time: Option<chrono::DateTime<Utc>>,
        start_time: Option<chrono::DateTime<Utc>>,
        finish_time: Option<chrono::DateTime<Utc>>,
        defer_time: Option<chrono::DateTime<Utc>>,
    }
    let row: Row = worker_job::table
        .filter(worker_job::_id.eq(id))
        .first(conn)
        .map_err(|_| crate::errors::TRK_027.with_data(id.to_string()))?;
    Ok(WorkerJob {
        meta: TrackerRowMeta {
            id: row._id,
            created: row._created,
            updated: row._updated,
            creator: row._creator,
            updater: row._updater,
            tenant: row._tenant,
            etag: Some(row._etag),
            deleted: row._deleted,
        },
        worker_id: row.worker_id.map(TrackerId),
        job_message: row.job_message,
        job_progress: row.job_progress,
        job_status: row.job_status.map(|s| JobStatus::from_db(&s)),
        job_try: row.job_try,
        score: row.score,
        queue_name: row.queue_name,
        function: row.function,
        args: row.args,
        kwargs: row.kwargs,
        result: row.result,
        err_message: row.err_message,
        err_trace: row.err_trace,
        enqueue_time: row.enqueue_time,
        start_time: row.start_time,
        finish_time: row.finish_time,
        defer_time: row.defer_time,
    })
}

fn insert_job_relation(
    conn: &mut diesel::pg::PgConnection,
    relation: &JobRelation,
) -> RiverbaseResult<()> {
    diesel::insert_into(job_relation::table)
        .values((
            job_relation::_id.eq(relation.meta.id),
            job_relation::_created.eq(relation.meta.created),
            job_relation::_updated.eq(relation.meta.updated),
            job_relation::_creator.eq(relation.meta.creator),
            job_relation::_updater.eq(relation.meta.updater),
            job_relation::_tenant.eq(relation.meta.tenant),
            job_relation::_etag.eq(relation.meta.etag.unwrap_or_else(uuid::Uuid::new_v4)),
            job_relation::_deleted.eq(relation.meta.deleted),
            job_relation::job_id.eq(relation.job_id.as_ref().map(|id| id.0)),
            job_relation::resource.eq(relation.resource.as_deref()),
            job_relation::resource_id.eq(relation.resource_id),
            job_relation::domain.eq(relation.domain.as_deref()),
        ))
        .execute(conn)
        .map_err(|e| crate::errors::TRK_029.with_data(e.to_string()))?;
    Ok(())
}

fn update_job_relation_row(
    conn: &mut diesel::pg::PgConnection,
    relation: &JobRelation,
) -> RiverbaseResult<()> {
    diesel::update(job_relation::table.filter(job_relation::_id.eq(relation.meta.id)))
        .set((
            job_relation::_updated.eq(relation.meta.updated),
            job_relation::_creator.eq(relation.meta.creator),
            job_relation::_updater.eq(relation.meta.updater),
            job_relation::_tenant.eq(relation.meta.tenant),
            job_relation::_etag.eq(relation.meta.etag.unwrap_or_else(uuid::Uuid::new_v4)),
            job_relation::_deleted.eq(relation.meta.deleted),
            job_relation::job_id.eq(relation.job_id.as_ref().map(|id| id.0)),
            job_relation::resource.eq(relation.resource.as_deref()),
            job_relation::resource_id.eq(relation.resource_id),
            job_relation::domain.eq(relation.domain.as_deref()),
        ))
        .execute(conn)
        .map_err(|e| crate::errors::TRK_031.with_data(e.to_string()))?;
    Ok(())
}

fn fetch_job_relation_row(
    conn: &mut diesel::pg::PgConnection,
    id: uuid::Uuid,
) -> RiverbaseResult<JobRelation> {
    #[derive(Queryable)]
    struct Row {
        _id: uuid::Uuid,
        _created: chrono::DateTime<Utc>,
        _updated: Option<chrono::DateTime<Utc>>,
        _creator: Option<uuid::Uuid>,
        _updater: Option<uuid::Uuid>,
        _tenant: Option<uuid::Uuid>,
        _etag: uuid::Uuid,
        _deleted: Option<chrono::DateTime<Utc>>,
        job_id: Option<uuid::Uuid>,
        resource: Option<String>,
        resource_id: Option<uuid::Uuid>,
        domain: Option<String>,
    }
    let row: Row = job_relation::table
        .filter(job_relation::_id.eq(id))
        .first(conn)
        .map_err(|_| crate::errors::TRK_033.with_data(id.to_string()))?;
    Ok(JobRelation {
        meta: TrackerRowMeta {
            id: row._id,
            created: row._created,
            updated: row._updated,
            creator: row._creator,
            updater: row._updater,
            tenant: row._tenant,
            etag: Some(row._etag),
            deleted: row._deleted,
        },
        job_id: row.job_id.map(TrackerId),
        resource: row.resource,
        resource_id: row.resource_id,
        domain: row.domain,
    })
}

fn apply_worker_update(worker: &mut Worker, patch: WorkerUpdate) {
    crate::tracker::patch::apply_worker_update(worker, patch);
}

fn apply_worker_job_update(job: &mut WorkerJob, patch: WorkerJobUpdate) {
    crate::tracker::patch::apply_worker_job_update(job, patch);
}

fn apply_job_relation_update(relation: &mut JobRelation, patch: JobRelationUpdate) {
    crate::tracker::patch::apply_job_relation_update(relation, patch);
}
