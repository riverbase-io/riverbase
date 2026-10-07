//! Audit and outbox retention ([DAT-08]).

use chrono::{Duration, Utc};
use diesel::pg::PgConnection;
use diesel::RunQueryDsl;
use serde::Deserialize;
use tracing::info;

use crate::datastore::error::DataResult;
use crate::datastore::postgres::prepare_migration_search_path;
use crate::datastore::postgres::AUDIT_MIGRATION_SCHEMA;

/// Per-channel retention windows in days ([DAT-08]).
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AuditRetentionConfig {
    /// Context log days.
    pub context_log_days: u32,
    /// Activity log days.
    pub activity_log_days: u32,
    /// Event log days.
    pub event_log_days: u32,
    /// Message log days.
    pub message_log_days: u32,
    /// Command log days.
    pub command_log_days: u32,
    /// Query log days.
    pub query_log_days: u32,
    /// Outbox days.
    pub outbox_days: u32,
}

impl Default for AuditRetentionConfig {
    fn default() -> Self {
        Self {
            context_log_days: 90,
            activity_log_days: 180,
            event_log_days: 365,
            message_log_days: 90,
            command_log_days: 730,
            query_log_days: 30,
            outbox_days: 14,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ChannelRetention {
    table: &'static str,
    days: u32,
}

/// Delete rows older than each channel's configured window.
pub fn purge_audit_logs(conn: &mut PgConnection, config: &AuditRetentionConfig) -> DataResult<u64> {
    prepare_migration_search_path(conn, AUDIT_MIGRATION_SCHEMA)?;
    let channels = [
        ChannelRetention {
            table: "context_log",
            days: config.context_log_days,
        },
        ChannelRetention {
            table: "activity_log",
            days: config.activity_log_days,
        },
        ChannelRetention {
            table: "event_log",
            days: config.event_log_days,
        },
        ChannelRetention {
            table: "message_log",
            days: config.message_log_days,
        },
        ChannelRetention {
            table: "command_log",
            days: config.command_log_days,
        },
        ChannelRetention {
            table: "query_log",
            days: config.query_log_days,
        },
        ChannelRetention {
            table: "outbox",
            days: config.outbox_days,
        },
    ];

    let mut deleted = 0_u64;
    for channel in channels {
        if channel.days == 0 {
            continue;
        }
        let cutoff = Utc::now() - Duration::days(i64::from(channel.days));
        let sql = format!(
            "DELETE FROM {AUDIT_MIGRATION_SCHEMA}.{} WHERE _created < '{cutoff}'",
            channel.table
        );
        let count = diesel::sql_query(sql)
            .execute(conn)
            .map_err(|e| crate::errors::DAT_085.with_data(e.to_string()))?;
        deleted += count as u64;
        info!(
            table = channel.table,
            retention_days = channel.days,
            deleted = count,
            "audit retention purge"
        );
    }
    Ok(deleted)
}
