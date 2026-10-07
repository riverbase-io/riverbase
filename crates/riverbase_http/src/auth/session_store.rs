//! Postgres-backed [`tower_sessions::SessionStore`].

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use diesel::sql_types::{Jsonb, Text, Timestamptz};
use diesel::{sql_query, QueryableByName};
use diesel_async::RunQueryDsl;
use riverbase_core::datastore::PgPool;
use serde_json::Value;
use time::OffsetDateTime;
use tower_sessions::session::{Id, Record};
use tower_sessions::session_store::{self, SessionStore};

const ENSURE_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS riverbase_http_session (
    id TEXT PRIMARY KEY,
    data JSONB NOT NULL,
    expiry_date TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_riverbase_http_session_expiry
    ON riverbase_http_session (expiry_date);
"#;

#[derive(Debug, QueryableByName)]
struct SessionRow {
    #[diesel(sql_type = Text)]
    id: String,
    #[diesel(sql_type = Jsonb)]
    data: Value,
    #[diesel(sql_type = Timestamptz)]
    expiry_date: DateTime<Utc>,
}

/// Session store persisted in Postgres (`riverbase_http_session`).
#[derive(Clone)]
pub struct PgSessionStore {
    pool: Arc<PgPool>,
}

impl fmt::Debug for PgSessionStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PgSessionStore").finish_non_exhaustive()
    }
}

impl PgSessionStore {
    /// Construct a new value.
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }

    /// Ensure schema.
    pub async fn ensure_schema(&self) -> session_store::Result<()> {
        let mut conn = self.pool.get().await.map_err(backend)?;
        for stmt in ENSURE_SCHEMA_SQL
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            sql_query(stmt).execute(&mut *conn).await.map_err(backend)?;
        }
        Ok(())
    }
}

fn backend<E: std::fmt::Display>(err: E) -> session_store::Error {
    session_store::Error::Backend(err.to_string())
}

fn encode_err<E: std::fmt::Display>(err: E) -> session_store::Error {
    session_store::Error::Encode(err.to_string())
}

fn decode_err<E: std::fmt::Display>(err: E) -> session_store::Error {
    session_store::Error::Decode(err.to_string())
}

fn id_to_string(id: &Id) -> String {
    id.to_string()
}

fn string_to_id(s: &str) -> session_store::Result<Id> {
    s.parse().map_err(decode_err)
}

fn offset_to_chrono(ts: OffsetDateTime) -> session_store::Result<DateTime<Utc>> {
    DateTime::from_timestamp(ts.unix_timestamp(), ts.nanosecond()).ok_or_else(|| {
        session_store::Error::Encode(format!("invalid session expiry timestamp: {ts}"))
    })
}

fn chrono_to_offset(ts: DateTime<Utc>) -> session_store::Result<OffsetDateTime> {
    OffsetDateTime::from_unix_timestamp(ts.timestamp())
        .map_err(decode_err)
        .map(|base| base + time::Duration::nanoseconds(i64::from(ts.timestamp_subsec_nanos())))
}

fn row_to_record(row: SessionRow) -> session_store::Result<Record> {
    let id = string_to_id(&row.id)?;
    let data: HashMap<String, Value> = serde_json::from_value(row.data).map_err(decode_err)?;
    let expiry_date = chrono_to_offset(row.expiry_date)?;
    Ok(Record {
        id,
        data,
        expiry_date,
    })
}

#[async_trait]
impl SessionStore for PgSessionStore {
    async fn create(&self, record: &mut Record) -> session_store::Result<()> {
        let mut conn = self.pool.get().await.map_err(backend)?;
        loop {
            let id = id_to_string(&record.id);
            let expiry = offset_to_chrono(record.expiry_date)?;
            let data = serde_json::to_value(&record.data).map_err(encode_err)?;
            let inserted = sql_query(
                r#"
                INSERT INTO riverbase_http_session (id, data, expiry_date)
                VALUES ($1, $2, $3)
                ON CONFLICT (id) DO NOTHING
                "#,
            )
            .bind::<Text, _>(&id)
            .bind::<Jsonb, _>(&data)
            .bind::<Timestamptz, _>(expiry)
            .execute(&mut *conn)
            .await
            .map_err(backend)?;
            if inserted == 1 {
                return Ok(());
            }
            record.id = Id::default();
        }
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        let id = id_to_string(&record.id);
        let expiry = offset_to_chrono(record.expiry_date)?;
        let data = serde_json::to_value(&record.data).map_err(encode_err)?;
        let mut conn = self.pool.get().await.map_err(backend)?;
        sql_query(
            r#"
            INSERT INTO riverbase_http_session (id, data, expiry_date)
            VALUES ($1, $2, $3)
            ON CONFLICT (id) DO UPDATE
            SET data = EXCLUDED.data,
                expiry_date = EXCLUDED.expiry_date
            "#,
        )
        .bind::<Text, _>(&id)
        .bind::<Jsonb, _>(&data)
        .bind::<Timestamptz, _>(expiry)
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
        Ok(())
    }

    async fn load(&self, session_id: &Id) -> session_store::Result<Option<Record>> {
        let id = id_to_string(session_id);
        let mut conn = self.pool.get().await.map_err(backend)?;
        let rows: Vec<SessionRow> = sql_query(
            r#"
            SELECT id, data, expiry_date
            FROM riverbase_http_session
            WHERE id = $1 AND expiry_date > NOW()
            "#,
        )
        .bind::<Text, _>(&id)
        .load(&mut *conn)
        .await
        .map_err(backend)?;
        rows.into_iter().next().map(row_to_record).transpose()
    }

    async fn delete(&self, session_id: &Id) -> session_store::Result<()> {
        let id = id_to_string(session_id);
        let mut conn = self.pool.get().await.map_err(backend)?;
        sql_query("DELETE FROM riverbase_http_session WHERE id = $1")
            .bind::<Text, _>(&id)
            .execute(&mut *conn)
            .await
            .map_err(backend)?;
        Ok(())
    }
}
