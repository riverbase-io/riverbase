use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::RunQueryDsl;

use super::idempotency_store::PostgresIdempotencyStore;
use super::schema::{
    activity_log, command_log, command_response, context_log, event_log, message_log, outbox,
    query_log,
};
use crate::base::{CommandId, RiverbaseResult};
use crate::datastore::postgres::{optional_command_connection, pool_connection, PgPool};
use crate::datastore::CommandUnitOfWork;
use crate::logstore::bundle::DomainLogStore;
use crate::logstore::config::AuditLogConfig;
use crate::logstore::model::{
    ActivityLogRecord, CommandLogRecord, ContextLogRecord, EventLogRecord, MessageLogRecord,
    OutboxRecord, QueryLogRecord, ResponseRecord,
};
use crate::logstore::outbox::OutboxStore;
use crate::logstore::response::ResponseLogStore;
use crate::logstore::store::{CommandStatusLogStore, LogStore};
use crate::logstore::util::new_log_id;
use crate::logstore::CommandLogStatus;

/// Postgres backends for all audit log types (shared pool).
#[derive(Clone)]
pub struct PostgresDomainLogStore {
    dbpool: PgPool,
}

impl PostgresDomainLogStore {
    /// Construct a new value.
    pub fn new(dbpool: PgPool) -> Self {
        Self { dbpool }
    }

    /// All audit channels enabled.
    pub fn into_bundle(self) -> DomainLogStore {
        self.into_bundle_with_config(&AuditLogConfig::default())
    }

    /// Apply per-channel on/off switches from configuration.
    pub fn into_bundle_with_config(self, config: &AuditLogConfig) -> DomainLogStore {
        let idempotency = PostgresIdempotencyStore::new(self.dbpool.clone());
        let s = Arc::new(self);
        DomainLogStore::from_config(
            config,
            s.clone() as Arc<dyn LogStore<ContextLogRecord>>,
            s.clone() as Arc<dyn CommandStatusLogStore>,
            s.clone() as Arc<dyn LogStore<EventLogRecord>>,
            s.clone() as Arc<dyn LogStore<MessageLogRecord>>,
            s.clone() as Arc<dyn LogStore<ActivityLogRecord>>,
            s.clone() as Arc<dyn LogStore<QueryLogRecord>>,
            s.clone() as Arc<dyn ResponseLogStore>,
            Arc::new(idempotency) as Arc<dyn crate::logstore::IdempotencyStore>,
            s as Arc<dyn OutboxStore>,
        )
    }

    async fn conn(
        &self,
        uow: Option<&CommandUnitOfWork>,
    ) -> RiverbaseResult<crate::datastore::postgres::PgConnectionGuard> {
        match optional_command_connection(uow, &self.dbpool).await {
            Ok(guard) => Ok(guard),
            // A finished transaction is not a pool checkout failure.
            Err(error) if error.errcode.as_str() == "DAT-028" => {
                Err(crate::errors::DAT_010.with_data(error.to_string()))
            }
            Err(error) => Err(error),
        }
    }

    async fn read_conn(&self) -> RiverbaseResult<crate::datastore::postgres::PgConnectionGuard> {
        pool_connection(&self.dbpool)
            .await
            .map_err(|e| crate::errors::DAT_010.with_data(e.to_string()))
    }
}

#[async_trait]
impl LogStore<ContextLogRecord> for PostgresDomainLogStore {
    async fn append(
        &self,
        uow: Option<&CommandUnitOfWork>,
        record: ContextLogRecord,
    ) -> RiverbaseResult<()> {
        let mut conn = self.conn(uow).await?;
        let roles = if record.iam_roles.is_empty() {
            None
        } else {
            Some(record.iam_roles.clone())
        };
        diesel::insert_into(context_log::table)
            .values((
                context_log::_id.eq(record.meta.id),
                context_log::_created.eq(record.meta.created),
                context_log::_creator.eq(record.meta.creator),
                context_log::domain.eq(record.domain),
                context_log::revision.eq(record.revision),
                context_log::realm.eq(record.realm),
                context_log::dataset_id.eq(record.dataset_id),
                context_log::request_id.eq(record.request_id),
                context_log::user_id.eq(record.user_id),
                context_log::profile_id.eq(record.profile_id),
                context_log::organization_id.eq(record.organization_id),
                context_log::iam_roles.eq(roles),
                context_log::session.eq(record.session),
                context_log::timestamp.eq(Some(record.timestamp)),
                context_log::transport.eq(Some(record.transport)),
                context_log::source.eq(Some(record.source)),
                context_log::headers.eq(record.headers),
                context_log::_tenant.eq(record.tenant),
            ))
            .execute(&mut conn)
            .await
            .map_err(|e| crate::errors::LOG_021.with_data(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl LogStore<CommandLogRecord> for PostgresDomainLogStore {
    async fn append(
        &self,
        uow: Option<&CommandUnitOfWork>,
        record: CommandLogRecord,
    ) -> RiverbaseResult<()> {
        let mut conn = self.conn(uow).await?;
        diesel::insert_into(command_log::table)
            .values((
                command_log::_id.eq(record.meta.id),
                command_log::_created.eq(record.meta.created),
                command_log::_creator.eq(record.meta.creator),
                command_log::domain.eq(record.domain),
                command_log::identifier.eq(record.identifier),
                command_log::resource.eq(record.resource),
                command_log::revision.eq(record.revision),
                command_log::command.eq(record.command),
                command_log::domain_sid.eq(record.domain_sid),
                command_log::domain_iid.eq(record.domain_iid),
                command_log::payload.eq(record.payload),
                command_log::context.eq(record.context),
                command_log::status.eq(Some(record.status)),
                command_log::_tenant.eq(record.tenant),
            ))
            .execute(&mut conn)
            .await
            .map_err(|e| crate::errors::LOG_004.with_data(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl CommandStatusLogStore for PostgresDomainLogStore {
    async fn set_status(
        &self,
        uow: Option<&CommandUnitOfWork>,
        command_id: uuid::Uuid,
        status: CommandLogStatus,
    ) -> RiverbaseResult<()> {
        let mut conn = self.conn(uow).await?;
        let updated = diesel::update(command_log::table.filter(command_log::_id.eq(command_id)))
            .set(command_log::status.eq(Some(status)))
            .execute(&mut conn)
            .await
            .map_err(|e| crate::errors::LOG_018.with_data(e.to_string()))?;
        if updated == 0 {
            return Err(crate::errors::LOG_017.with_data(command_id.to_string()));
        }
        Ok(())
    }
}

#[async_trait]
impl LogStore<EventLogRecord> for PostgresDomainLogStore {
    async fn append(
        &self,
        uow: Option<&CommandUnitOfWork>,
        record: EventLogRecord,
    ) -> RiverbaseResult<()> {
        let mut conn = self.conn(uow).await?;
        diesel::insert_into(event_log::table)
            .values((
                event_log::_id.eq(record.meta.id),
                event_log::_created.eq(record.meta.created),
                event_log::_creator.eq(record.meta.creator),
                event_log::domain.eq(record.domain),
                event_log::event.eq(record.event),
                event_log::identifier.eq(record.identifier),
                event_log::resource.eq(record.resource),
                event_log::src_cmd.eq(record.src_cmd),
                event_log::args.eq(record.args),
                event_log::data.eq(record.data),
                event_log::_tenant.eq(record.tenant),
            ))
            .execute(&mut conn)
            .await
            .map_err(|e| crate::errors::LOG_007.with_data(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl LogStore<MessageLogRecord> for PostgresDomainLogStore {
    async fn append(
        &self,
        uow: Option<&CommandUnitOfWork>,
        record: MessageLogRecord,
    ) -> RiverbaseResult<()> {
        let mut conn = self.conn(uow).await?;
        diesel::insert_into(message_log::table)
            .values((
                message_log::_id.eq(record.meta.id),
                message_log::_created.eq(record.meta.created),
                message_log::_creator.eq(record.meta.creator),
                message_log::domain.eq(record.domain),
                message_log::src_cmd.eq(record.src_cmd),
                message_log::message.eq(record.message),
                message_log::data.eq(record.data),
                message_log::_tenant.eq(record.tenant),
            ))
            .execute(&mut conn)
            .await
            .map_err(|e| crate::errors::LOG_010.with_data(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl LogStore<ActivityLogRecord> for PostgresDomainLogStore {
    async fn append(
        &self,
        uow: Option<&CommandUnitOfWork>,
        record: ActivityLogRecord,
    ) -> RiverbaseResult<()> {
        let mut conn = self.conn(uow).await?;
        diesel::insert_into(activity_log::table)
            .values((
                activity_log::_id.eq(record.meta.id),
                activity_log::_created.eq(record.meta.created),
                activity_log::_creator.eq(record.meta.creator),
                activity_log::_source.eq(record.source),
                activity_log::domain.eq(record.domain),
                activity_log::identifier.eq(record.identifier),
                activity_log::resource.eq(record.resource),
                activity_log::domain_sid.eq(record.domain_sid),
                activity_log::domain_iid.eq(record.domain_iid),
                activity_log::message.eq(record.message),
                activity_log::msgtype.eq(Some(record.msgtype)),
                activity_log::msglabel.eq(record.msglabel),
                activity_log::context.eq(record.context),
                activity_log::src_cmd.eq(record.src_cmd),
                activity_log::src_evt.eq(record.src_evt),
                activity_log::data.eq(record.data),
                activity_log::code.eq(record.code),
                activity_log::_tenant.eq(record.tenant),
            ))
            .execute(&mut conn)
            .await
            .map_err(|e| crate::errors::LOG_013.with_data(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl LogStore<QueryLogRecord> for PostgresDomainLogStore {
    async fn append(
        &self,
        uow: Option<&CommandUnitOfWork>,
        record: QueryLogRecord,
    ) -> RiverbaseResult<()> {
        let mut conn = self.conn(uow).await?;
        diesel::insert_into(query_log::table)
            .values((
                query_log::_id.eq(record.meta.id),
                query_log::_created.eq(record.meta.created),
                query_log::_creator.eq(record.meta.creator),
                query_log::domain.eq(record.domain),
                query_log::resource.eq(record.resource),
                query_log::access.eq(record.access),
                query_log::identifier.eq(record.identifier),
                query_log::domain_sid.eq(record.domain_sid),
                query_log::domain_iid.eq(record.domain_iid),
                query_log::request.eq(record.request),
                query_log::context.eq(record.context),
                query_log::status.eq(Some(record.status)),
                query_log::result_count.eq(record.result_count),
                query_log::error_code.eq(record.error_code),
                query_log::_tenant.eq(record.tenant),
            ))
            .execute(&mut conn)
            .await
            .map_err(|e| crate::errors::LOG_016.with_data(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl LogStore<ResponseRecord> for PostgresDomainLogStore {
    async fn append(
        &self,
        uow: Option<&CommandUnitOfWork>,
        record: ResponseRecord,
    ) -> RiverbaseResult<()> {
        let mut conn = self.conn(uow).await?;
        let now = Utc::now();
        diesel::insert_into(command_response::table)
            .values((
                command_response::cmd_id.eq(record.cmd_id.0),
                command_response::payload.eq(record.payload),
                command_response::created_at.eq(now),
                command_response::updated_at.eq(now),
            ))
            .on_conflict(command_response::cmd_id)
            .do_update()
            .set((
                command_response::payload.eq(diesel::upsert::excluded(command_response::payload)),
                command_response::updated_at.eq(now),
            ))
            .execute(&mut conn)
            .await
            .map_err(|e| crate::errors::LOG_019.with_data(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl ResponseLogStore for PostgresDomainLogStore {
    async fn get(&self, cmd_id: &CommandId) -> RiverbaseResult<ResponseRecord> {
        let mut conn = self.read_conn().await?;
        let payload = command_response::table
            .filter(command_response::cmd_id.eq(&cmd_id.0))
            .select(command_response::payload)
            .first::<serde_json::Value>(&mut conn)
            .await
            .map_err(|e| match e {
                diesel::result::Error::NotFound => {
                    crate::errors::LOG_015.with_data(cmd_id.0.clone())
                }
                other => crate::errors::LOG_020.with_data(other.to_string()),
            })?;
        Ok(ResponseRecord {
            cmd_id: cmd_id.clone(),
            payload,
        })
    }
}

#[async_trait]
impl OutboxStore for PostgresDomainLogStore {
    async fn enqueue(
        &self,
        uow: Option<&CommandUnitOfWork>,
        record: OutboxRecord,
    ) -> RiverbaseResult<()> {
        let mut conn = self.conn(uow).await?;
        diesel::insert_into(outbox::table)
            .values((
                outbox::_id.eq(record.id),
                outbox::_created.eq(record.created),
                outbox::src_cmd.eq(record.src_cmd),
                outbox::topic.eq(record.topic),
                outbox::payload.eq(record.payload),
                outbox::status.eq("pending"),
                outbox::attempts.eq(record.attempts),
                outbox::next_attempt_at.eq(record.created),
            ))
            .on_conflict(outbox::_id)
            .do_nothing()
            .execute(&mut conn)
            .await
            .map_err(|e| crate::errors::OUT_002.with_data(e.to_string()))?;
        Ok(())
    }

    async fn claim_due(&self, limit: i64, lease: Duration) -> RiverbaseResult<Vec<OutboxRecord>> {
        #[derive(diesel::QueryableByName)]
        struct ClaimedRow {
            #[diesel(sql_type = diesel::sql_types::Uuid)]
            id: uuid::Uuid,
            #[diesel(sql_type = diesel::sql_types::Timestamptz)]
            created: DateTime<Utc>,
            #[diesel(sql_type = diesel::sql_types::Uuid)]
            src_cmd: uuid::Uuid,
            #[diesel(sql_type = diesel::sql_types::Text)]
            topic: String,
            #[diesel(sql_type = diesel::sql_types::Jsonb)]
            payload: serde_json::Value,
            #[diesel(sql_type = diesel::sql_types::Integer)]
            attempts: i32,
        }

        let mut conn = self.read_conn().await?;
        let lease_seconds = i64::try_from(lease.as_secs()).unwrap_or(i64::MAX);
        let rows = diesel::sql_query(
            r#"
            WITH due AS (
                SELECT _id
                FROM riverbase_audit.outbox
                WHERE status IN ('pending', 'publishing')
                  AND next_attempt_at <= now()
                ORDER BY _created
                FOR UPDATE SKIP LOCKED
                LIMIT $1
            )
            UPDATE riverbase_audit.outbox AS queued
            SET status = 'publishing',
                attempts = queued.attempts + 1,
                next_attempt_at = now() + ($2::double precision * interval '1 second')
            FROM due
            WHERE queued._id = due._id
            RETURNING queued._id AS id,
                      queued._created AS created,
                      queued.src_cmd,
                      queued.topic,
                      queued.payload,
                      queued.attempts
            "#,
        )
        .bind::<diesel::sql_types::BigInt, _>(limit.max(0))
        .bind::<diesel::sql_types::BigInt, _>(lease_seconds)
        .load::<ClaimedRow>(&mut conn)
        .await
        .map_err(|e| crate::errors::OUT_003.with_data(e.to_string()))?;
        Ok(rows
            .into_iter()
            .map(|row| OutboxRecord {
                id: row.id,
                created: row.created,
                src_cmd: row.src_cmd,
                topic: row.topic,
                payload: row.payload,
                attempts: row.attempts,
            })
            .collect())
    }

    async fn mark_published(&self, id: uuid::Uuid) -> RiverbaseResult<()> {
        let mut conn = self.read_conn().await?;
        let updated = diesel::update(outbox::table.filter(outbox::_id.eq(id)))
            .set((
                outbox::status.eq("published"),
                outbox::published_at.eq(Some(Utc::now())),
                outbox::last_error.eq::<Option<String>>(None),
            ))
            .execute(&mut conn)
            .await
            .map_err(|e| crate::errors::OUT_004.with_data(e.to_string()))?;
        if updated == 0 {
            return Err(crate::errors::OUT_001.with_data(id.to_string()));
        }
        Ok(())
    }

    async fn mark_failed(
        &self,
        id: uuid::Uuid,
        error: &str,
        retry_after: Duration,
        max_attempts: i32,
    ) -> RiverbaseResult<()> {
        let mut conn = self.read_conn().await?;
        let retry_seconds = i64::try_from(retry_after.as_secs()).unwrap_or(i64::MAX);
        let updated = diesel::sql_query(
            r#"
            UPDATE riverbase_audit.outbox
            SET status = CASE
                    WHEN attempts >= $4 THEN 'dead_letter'
                    ELSE 'pending'
                END,
                next_attempt_at = now() + ($3::double precision * interval '1 second'),
                last_error = $2
            WHERE _id = $1
            "#,
        )
        .bind::<diesel::sql_types::Uuid, _>(id)
        .bind::<diesel::sql_types::Text, _>(error)
        .bind::<diesel::sql_types::BigInt, _>(retry_seconds)
        .bind::<diesel::sql_types::Integer, _>(max_attempts)
        .execute(&mut conn)
        .await
        .map_err(|e| crate::errors::OUT_005.with_data(e.to_string()))?;
        if updated == 0 {
            return Err(crate::errors::OUT_001.with_data(id.to_string()));
        }
        Ok(())
    }
}

/// New id.
pub fn new_id() -> String {
    new_log_id().to_string()
}
