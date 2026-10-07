//! PostgreSQL LISTEN/NOTIFY message bus ([RUN-01]).

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::Mutex;
use tokio_postgres::Client;
use uuid::Uuid;

use crate::base::RiverbaseResult;
use crate::command::MessageBus;

/// Payloads larger than this are spilled to `riverbase_bus_notify_payload` (PG NOTIFY limit).
pub const MAX_NOTIFY_BYTES: usize = 8_000;

const SPILL_PREFIX: &str = "spill:";

const ENSURE_SPILL_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS riverbase_bus_notify_payload (
    id UUID PRIMARY KEY,
    payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
)
"#;

/// PostgreSQL LISTEN/NOTIFY-backed bus (publish + subscribe for outbox / SSE / RTC).
pub struct PgMessageBus {
    dsn: String,
    publish: Mutex<Client>,
}

impl PgMessageBus {
    /// Connect.
    pub async fn connect(db_url: &str) -> RiverbaseResult<Self> {
        let dsn = normalize_pg_url(db_url);
        let (client, connection) = connect_pair(&dsn).await?;
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::error!(error = %e, "postgres bus connection closed");
            }
        });
        client
            .execute(ENSURE_SPILL_TABLE, &[])
            .await
            .map_err(|e| crate::errors::TRN_021.with_data(pg_error_detail(&e)))?;
        Ok(Self {
            dsn,
            publish: Mutex::new(client),
        })
    }

    /// Dsn.
    pub fn dsn(&self) -> &str {
        &self.dsn
    }

    /// Channel name.
    pub fn channel_name(topic: &str) -> String {
        pg_channel(topic)
    }

    /// Resolve notify payload.
    pub async fn resolve_notify_payload(&self, raw: &str) -> RiverbaseResult<Value> {
        if let Some(id_str) = raw.strip_prefix(SPILL_PREFIX) {
            let id = Uuid::parse_str(id_str)
                .map_err(|e| crate::errors::TRN_023.with_data(e.to_string()))?;
            let client = self.publish.lock().await;
            let row = client
                .query_opt(
                    "SELECT payload FROM riverbase_bus_notify_payload WHERE id = $1",
                    &[&id],
                )
                .await
                .map_err(|e| crate::errors::TRN_024.with_data(pg_error_detail(&e)))?;
            let Some(row) = row else {
                return Err(crate::errors::TRN_025.with_data(id.to_string()).into());
            };
            let value: Value = row.get(0);
            let _ = client
                .execute(
                    "DELETE FROM riverbase_bus_notify_payload WHERE id = $1",
                    &[&id],
                )
                .await;
            return Ok(value);
        }
        serde_json::from_str(raw)
            .map_err(|e| crate::errors::TRN_026.with_data(e.to_string()).into())
    }
}

#[async_trait]
impl MessageBus for PgMessageBus {
    async fn publish(&self, topic: &str, payload: Value) -> RiverbaseResult<()> {
        let channel = pg_channel(topic);
        let encoded = serde_json::to_string(&payload)
            .map_err(|e| crate::errors::TRN_027.with_data(e.to_string()))?;
        let client = self.publish.lock().await;
        let notify_payload = if encoded.len() > MAX_NOTIFY_BYTES {
            let id = Uuid::new_v4();
            client
                .execute(
                    "INSERT INTO riverbase_bus_notify_payload (id, payload) VALUES ($1, $2)",
                    &[&id, &payload],
                )
                .await
                .map_err(|e| crate::errors::TRN_028.with_data(pg_error_detail(&e)))?;
            format!("{SPILL_PREFIX}{id}")
        } else {
            encoded
        };
        client
            .execute("SELECT pg_notify($1, $2)", &[&channel, &notify_payload])
            .await
            .map_err(|e| crate::errors::TRN_029.with_data(pg_error_detail(&e)))?;
        Ok(())
    }
}

pub(crate) async fn connect_pair(
    dsn: &str,
) -> RiverbaseResult<(
    Client,
    tokio_postgres::Connection<tokio_postgres::Socket, tokio_postgres::tls::NoTlsStream>,
)> {
    tokio_postgres::connect(dsn, tokio_postgres::NoTls)
        .await
        .map_err(|e| crate::errors::TRN_020.with_data(pg_error_detail(&e)).into())
}

/// Walk `Display` plus `Error::source` so tokio-postgres `Kind::Db` ("db error")
/// keeps the server message that lives only on the cause.
pub(crate) fn pg_error_detail(err: &tokio_postgres::Error) -> String {
    error_chain_detail(err)
}

fn error_chain_detail(err: &(dyn std::error::Error + '_)) -> String {
    let mut detail = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        detail.push_str(": ");
        detail.push_str(&cause.to_string());
        source = cause.source();
    }
    detail
}

fn normalize_pg_url(url: &str) -> String {
    let trimmed = url.trim();
    if let Some(rest) = trimmed.strip_prefix("postgresql+asyncpg://") {
        format!("postgresql://{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("postgres://") {
        format!("postgresql://{rest}")
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn pg_channel(topic: &str) -> String {
    topic
        .chars()
        .map(|c| match c {
            '.' | '-' | '/' | ':' => '_',
            c if c.is_ascii_alphanumeric() || c == '_' => c.to_ascii_lowercase(),
            _ => '_',
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::fmt;

    use super::error_chain_detail;

    #[derive(Debug)]
    struct Chain {
        msg: &'static str,
        source: Option<Box<dyn Error + Send + Sync>>,
    }

    impl fmt::Display for Chain {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.msg)
        }
    }

    impl Error for Chain {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            self.source.as_ref().map(|e| e.as_ref() as _)
        }
    }

    #[test]
    fn error_chain_detail_includes_nested_sources() {
        let err = Chain {
            msg: "db error",
            source: Some(Box::new(Chain {
                msg: "FATAL: password authentication failed",
                source: Some(Box::new(Chain {
                    msg: "detail: role does not exist",
                    source: None,
                })),
            })),
        };
        assert_eq!(
            error_chain_detail(&err),
            "db error: FATAL: password authentication failed: detail: role does not exist"
        );
    }

    #[test]
    fn error_chain_detail_without_source_is_display() {
        let err = Chain {
            msg: "invalid connection string",
            source: None,
        };
        assert_eq!(error_chain_detail(&err), "invalid connection string");
    }
}
