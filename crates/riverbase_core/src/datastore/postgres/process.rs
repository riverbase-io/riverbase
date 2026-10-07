use async_trait::async_trait;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use serde_json::Value;
use uuid::Uuid;

use super::{optional_command_connection, pool_connection, PgPool};
use crate::base::RiverbaseResult;
use crate::command::{ProcessManagerStore, ProcessState, ProcessStatus};
use crate::datastore::CommandUnitOfWork;

mod process_schema {
    #![allow(missing_docs)]

    diesel::table! {
        use diesel::sql_types::*;

        riverbase_workflow.process_manager (_id) {
            _id -> Uuid,
            workflow_type -> Text,
            correlation_key -> Text,
            state -> Jsonb,
            status -> Text,
            completed_steps -> Array<Text>,
            version -> BigInt,
            attempts -> Integer,
            next_attempt_at -> Nullable<Timestamptz>,
            last_error -> Nullable<Jsonb>,
            created_at -> Timestamptz,
            updated_at -> Timestamptz,
        }
    }
}

use process_schema::process_manager;

#[derive(Queryable, Selectable)]
#[diesel(table_name = process_manager)]
struct ProcessRow {
    _id: Uuid,
    workflow_type: String,
    correlation_key: String,
    state: Value,
    status: String,
    completed_steps: Vec<String>,
    version: i64,
    attempts: i32,
    next_attempt_at: Option<DateTime<Utc>>,
    last_error: Option<Value>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<ProcessRow> for ProcessState {
    type Error = crate::base::RiverbaseError;

    fn try_from(row: ProcessRow) -> Result<Self, Self::Error> {
        let status = match row.status.as_str() {
            "pending" => ProcessStatus::Pending,
            "running" => ProcessStatus::Running,
            "retrying" => ProcessStatus::Retrying,
            "compensating" => ProcessStatus::Compensating,
            "compensated" => ProcessStatus::Compensated,
            "failed" => ProcessStatus::Failed,
            "completed" => ProcessStatus::Completed,
            value => {
                return Err(crate::errors::PCS_004.with_data(value.to_string()));
            }
        };
        Ok(Self {
            id: row._id,
            workflow_type: row.workflow_type,
            correlation_key: row.correlation_key,
            state: row.state,
            status,
            completed_steps: row.completed_steps,
            version: row.version,
            attempts: row.attempts,
            next_attempt_at: row.next_attempt_at,
            last_error: row.last_error,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

#[derive(Clone)]
/// Postgres process manager store structure.
pub struct PostgresProcessManagerStore {
    pool: PgPool,
}

impl PostgresProcessManagerStore {
    /// Construct a Postgres-backed process-manager store.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ProcessManagerStore for PostgresProcessManagerStore {
    async fn create(
        &self,
        uow: Option<&CommandUnitOfWork>,
        process: ProcessState,
    ) -> RiverbaseResult<ProcessState> {
        let mut conn = optional_command_connection(uow, &self.pool)
            .await
            .map_err(process_storage_error)?;
        let result = diesel::insert_into(process_manager::table)
            .values((
                process_manager::_id.eq(process.id),
                process_manager::workflow_type.eq(&process.workflow_type),
                process_manager::correlation_key.eq(&process.correlation_key),
                process_manager::state.eq(&process.state),
                process_manager::status.eq(process.status.as_str()),
                process_manager::completed_steps.eq(&process.completed_steps),
                process_manager::version.eq(process.version),
                process_manager::attempts.eq(process.attempts),
                process_manager::next_attempt_at.eq(process.next_attempt_at),
                process_manager::last_error.eq(&process.last_error),
                process_manager::created_at.eq(process.created_at),
                process_manager::updated_at.eq(process.updated_at),
            ))
            .execute(&mut conn)
            .await;
        match result {
            Ok(_) => Ok(process),
            Err(diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::UniqueViolation,
                _,
            )) => Err(crate::errors::PCS_001.with_data(format!(
                "{}:{}",
                process.workflow_type, process.correlation_key
            ))),
            Err(error) => Err(process_storage_error(error)),
        }
    }

    async fn load(&self, id: Uuid) -> RiverbaseResult<Option<ProcessState>> {
        let mut conn = pool_connection(&self.pool)
            .await
            .map_err(process_storage_error)?;
        process_manager::table
            .filter(process_manager::_id.eq(id))
            .select(ProcessRow::as_select())
            .first::<ProcessRow>(&mut conn)
            .await
            .optional()
            .map_err(process_storage_error)?
            .map(TryInto::try_into)
            .transpose()
    }

    async fn load_by_correlation(
        &self,
        workflow_type: &str,
        correlation_key: &str,
    ) -> RiverbaseResult<Option<ProcessState>> {
        let mut conn = pool_connection(&self.pool)
            .await
            .map_err(process_storage_error)?;
        process_manager::table
            .filter(process_manager::workflow_type.eq(workflow_type))
            .filter(process_manager::correlation_key.eq(correlation_key))
            .select(ProcessRow::as_select())
            .first::<ProcessRow>(&mut conn)
            .await
            .optional()
            .map_err(process_storage_error)?
            .map(TryInto::try_into)
            .transpose()
    }

    async fn save(
        &self,
        uow: Option<&CommandUnitOfWork>,
        mut process: ProcessState,
        expected_version: i64,
    ) -> RiverbaseResult<ProcessState> {
        let mut conn = optional_command_connection(uow, &self.pool)
            .await
            .map_err(process_storage_error)?;
        process.version = expected_version + 1;
        process.updated_at = Utc::now();
        let updated = diesel::update(
            process_manager::table
                .filter(process_manager::_id.eq(process.id))
                .filter(process_manager::version.eq(expected_version)),
        )
        .set((
            process_manager::state.eq(&process.state),
            process_manager::status.eq(process.status.as_str()),
            process_manager::completed_steps.eq(&process.completed_steps),
            process_manager::version.eq(process.version),
            process_manager::attempts.eq(process.attempts),
            process_manager::next_attempt_at.eq(process.next_attempt_at),
            process_manager::last_error.eq(&process.last_error),
            process_manager::updated_at.eq(process.updated_at),
        ))
        .execute(&mut conn)
        .await
        .map_err(process_storage_error)?;
        if updated != 1 {
            return Err(crate::errors::PCS_003.with_data(serde_json::json!({
                "id": process.id,
                "expected_version": expected_version,
            })));
        }
        Ok(process)
    }

    async fn claim_due(&self, limit: i64) -> RiverbaseResult<Vec<ProcessState>> {
        #[derive(QueryableByName)]
        struct ClaimedRow {
            #[diesel(sql_type = diesel::sql_types::Uuid)]
            id: Uuid,
            #[diesel(sql_type = diesel::sql_types::Text)]
            workflow_type: String,
            #[diesel(sql_type = diesel::sql_types::Text)]
            correlation_key: String,
            #[diesel(sql_type = diesel::sql_types::Jsonb)]
            state: Value,
            #[diesel(sql_type = diesel::sql_types::Text)]
            status: String,
            #[diesel(sql_type = diesel::sql_types::Array<diesel::sql_types::Text>)]
            completed_steps: Vec<String>,
            #[diesel(sql_type = diesel::sql_types::BigInt)]
            version: i64,
            #[diesel(sql_type = diesel::sql_types::Integer)]
            attempts: i32,
            #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Timestamptz>)]
            next_attempt_at: Option<DateTime<Utc>>,
            #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Jsonb>)]
            last_error: Option<Value>,
            #[diesel(sql_type = diesel::sql_types::Timestamptz)]
            created_at: DateTime<Utc>,
            #[diesel(sql_type = diesel::sql_types::Timestamptz)]
            updated_at: DateTime<Utc>,
        }

        let mut conn = pool_connection(&self.pool)
            .await
            .map_err(process_storage_error)?;
        let rows = diesel::sql_query(
            r#"
            WITH due AS (
                SELECT _id
                FROM riverbase_workflow.process_manager
                WHERE status IN ('pending', 'retrying', 'compensating', 'running')
                  AND next_attempt_at <= now()
                ORDER BY next_attempt_at, created_at
                FOR UPDATE SKIP LOCKED
                LIMIT $1
            )
            UPDATE riverbase_workflow.process_manager AS process
            SET status = 'running',
                attempts = process.attempts + 1,
                version = process.version + 1,
                next_attempt_at = now() + interval '30 seconds',
                updated_at = now()
            FROM due
            WHERE process._id = due._id
            RETURNING process._id AS id,
                      process.workflow_type,
                      process.correlation_key,
                      process.state,
                      process.status,
                      process.completed_steps,
                      process.version,
                      process.attempts,
                      process.next_attempt_at,
                      process.last_error,
                      process.created_at,
                      process.updated_at
            "#,
        )
        .bind::<diesel::sql_types::BigInt, _>(limit.max(0))
        .load::<ClaimedRow>(&mut conn)
        .await
        .map_err(process_storage_error)?;
        rows.into_iter()
            .map(|row| {
                ProcessRow {
                    _id: row.id,
                    workflow_type: row.workflow_type,
                    correlation_key: row.correlation_key,
                    state: row.state,
                    status: row.status,
                    completed_steps: row.completed_steps,
                    version: row.version,
                    attempts: row.attempts,
                    next_attempt_at: row.next_attempt_at,
                    last_error: row.last_error,
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                }
                .try_into()
            })
            .collect()
    }
}

fn process_storage_error(error: impl std::fmt::Display) -> crate::base::RiverbaseError {
    crate::errors::PCS_005.with_data(error.to_string())
}
