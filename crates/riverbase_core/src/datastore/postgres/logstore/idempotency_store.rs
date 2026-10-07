use async_trait::async_trait;
use chrono::Utc;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use serde_json::Value;
use uuid::Uuid;

use super::schema::idempotency_key;
use crate::base::{CommandId, RiverbaseResult};
use crate::datastore::postgres::{optional_command_connection, pool_connection, PgPool};
use crate::datastore::CommandUnitOfWork;
use crate::logstore::idempotency::{
    ClaimOutcome, IdempotencyClaim, IdempotencyScope, IdempotencyStore, DEFAULT_LEASE,
    STATUS_COMPLETED, STATUS_FAILED, STATUS_IN_FLIGHT,
};

/// Postgres-backed idempotency store (shared pool with audit logstore).
#[derive(Clone)]
pub struct PostgresIdempotencyStore {
    dbpool: PgPool,
}

impl PostgresIdempotencyStore {
    /// Construct a new value.
    pub fn new(dbpool: PgPool) -> Self {
        Self { dbpool }
    }

    async fn conn(
        &self,
        uow: Option<&CommandUnitOfWork>,
    ) -> RiverbaseResult<crate::datastore::postgres::PgConnectionGuard> {
        optional_command_connection(uow, &self.dbpool)
            .await
            .map_err(|e| crate::errors::DAT_010.with_data(e.to_string()))
    }

    async fn read_conn(&self) -> RiverbaseResult<crate::datastore::postgres::PgConnectionGuard> {
        pool_connection(&self.dbpool)
            .await
            .map_err(|e| crate::errors::DAT_010.with_data(e.to_string()))
    }
}

#[async_trait]
impl IdempotencyStore for PostgresIdempotencyStore {
    async fn claim(
        &self,
        scope: &IdempotencyScope,
        actor: Option<Uuid>,
        request_hash: &str,
    ) -> RiverbaseResult<ClaimOutcome> {
        let mut conn = self.read_conn().await?;
        let now = Utc::now();
        let claim = IdempotencyClaim::new();
        let lease_expires_at = now + chrono::Duration::from_std(DEFAULT_LEASE).unwrap();

        let inserted = diesel::insert_into(idempotency_key::table)
            .values((
                idempotency_key::key.eq(&scope.key),
                idempotency_key::namespace.eq(&scope.namespace),
                idempotency_key::command.eq(&scope.command),
                idempotency_key::actor.eq(actor),
                idempotency_key::request_hash.eq(request_hash),
                idempotency_key::status.eq(STATUS_IN_FLIGHT),
                idempotency_key::created_at.eq(now),
                idempotency_key::owner_token.eq(Some(claim.owner_token)),
                idempotency_key::lease_expires_at.eq(Some(lease_expires_at)),
            ))
            .on_conflict((
                idempotency_key::namespace,
                idempotency_key::command,
                idempotency_key::key,
            ))
            .do_nothing()
            .execute(&mut conn)
            .await
            .map_err(|e| crate::errors::IDM_001.with_data(e.to_string()))?;

        if inserted > 0 {
            return Ok(ClaimOutcome::Fresh(claim));
        }

        #[derive(Queryable, Selectable)]
        #[diesel(table_name = idempotency_key)]
        struct Row {
            request_hash: String,
            status: String,
            response: Option<Value>,
            lease_expires_at: Option<chrono::DateTime<Utc>>,
            error: Option<Value>,
        }

        let row: Option<Row> = idempotency_key::table
            .filter(idempotency_key::namespace.eq(&scope.namespace))
            .filter(idempotency_key::command.eq(&scope.command))
            .filter(idempotency_key::key.eq(&scope.key))
            .select(Row::as_select())
            .first(&mut conn)
            .await
            .optional()
            .map_err(|e| crate::errors::IDM_002.with_data(e.to_string()))?;

        let Some(row) = row else {
            return Ok(ClaimOutcome::InFlight);
        };

        if row.request_hash != request_hash {
            return Ok(ClaimOutcome::Mismatch);
        }

        Ok(match row.status.as_str() {
            STATUS_COMPLETED => ClaimOutcome::Completed(row.response.unwrap_or(Value::Null)),
            STATUS_FAILED => ClaimOutcome::Failed(row.error.unwrap_or(Value::Null)),
            STATUS_IN_FLIGHT
                if row
                    .lease_expires_at
                    .is_some_and(|lease_expires_at| lease_expires_at > now) =>
            {
                ClaimOutcome::InFlight
            }
            STATUS_IN_FLIGHT => {
                let reclaimed = diesel::update(
                    idempotency_key::table
                        .filter(idempotency_key::namespace.eq(&scope.namespace))
                        .filter(idempotency_key::command.eq(&scope.command))
                        .filter(idempotency_key::key.eq(&scope.key))
                        .filter(idempotency_key::request_hash.eq(request_hash))
                        .filter(idempotency_key::status.eq(STATUS_IN_FLIGHT))
                        .filter(
                            idempotency_key::lease_expires_at
                                .is_null()
                                .or(idempotency_key::lease_expires_at.le(now)),
                        ),
                )
                .set((
                    idempotency_key::owner_token.eq(Some(claim.owner_token)),
                    idempotency_key::lease_expires_at.eq(Some(lease_expires_at)),
                ))
                .execute(&mut conn)
                .await
                .map_err(|e| crate::errors::IDM_006.with_data(e.to_string()))?;
                if reclaimed == 1 {
                    ClaimOutcome::Fresh(claim)
                } else {
                    ClaimOutcome::InFlight
                }
            }
            _ => ClaimOutcome::InFlight,
        })
    }

    async fn complete(
        &self,
        uow: Option<&CommandUnitOfWork>,
        scope: &IdempotencyScope,
        claim: &IdempotencyClaim,
        cmd_id: &CommandId,
        response: Value,
    ) -> RiverbaseResult<()> {
        let mut conn = self.conn(uow).await?;
        let now = Utc::now();
        let updated = diesel::update(
            idempotency_key::table
                .filter(idempotency_key::namespace.eq(&scope.namespace))
                .filter(idempotency_key::command.eq(&scope.command))
                .filter(idempotency_key::key.eq(&scope.key))
                .filter(idempotency_key::owner_token.eq(Some(claim.owner_token)))
                .filter(idempotency_key::status.eq(STATUS_IN_FLIGHT)),
        )
        .set((
            idempotency_key::status.eq(STATUS_COMPLETED),
            idempotency_key::response.eq(Some(response)),
            idempotency_key::cmd_id.eq(Some(cmd_id.0.clone())),
            idempotency_key::completed_at.eq(Some(now)),
        ))
        .execute(&mut conn)
        .await
        .map_err(|e| crate::errors::IDM_003.with_data(e.to_string()))?;
        if updated != 1 {
            return Err(crate::errors::IDM_005.with_data(scope.key.clone()));
        }
        Ok(())
    }

    async fn fail(
        &self,
        uow: Option<&CommandUnitOfWork>,
        scope: &IdempotencyScope,
        claim: &IdempotencyClaim,
        cmd_id: &CommandId,
        error: Value,
    ) -> RiverbaseResult<()> {
        let mut conn = self.conn(uow).await?;
        let now = Utc::now();
        let updated = diesel::update(
            idempotency_key::table
                .filter(idempotency_key::namespace.eq(&scope.namespace))
                .filter(idempotency_key::command.eq(&scope.command))
                .filter(idempotency_key::key.eq(&scope.key))
                .filter(idempotency_key::owner_token.eq(Some(claim.owner_token)))
                .filter(idempotency_key::status.eq(STATUS_IN_FLIGHT)),
        )
        .set((
            idempotency_key::status.eq(STATUS_FAILED),
            idempotency_key::cmd_id.eq(Some(cmd_id.0.clone())),
            idempotency_key::failed_at.eq(Some(now)),
            idempotency_key::error.eq(Some(error)),
            idempotency_key::lease_expires_at.eq::<Option<chrono::DateTime<Utc>>>(None),
        ))
        .execute(&mut conn)
        .await
        .map_err(|e| crate::errors::IDM_004.with_data(e.to_string()))?;
        if updated != 1 {
            return Err(crate::errors::IDM_005.with_data(scope.key.clone()));
        }
        Ok(())
    }
}
