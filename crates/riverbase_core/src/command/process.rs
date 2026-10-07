use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::base::RiverbaseResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Process Status enumeration.
pub enum ProcessStatus {
    /// Pending.
    Pending,
    /// Running.
    Running,
    /// Retrying.
    Retrying,
    /// Compensating.
    Compensating,
    /// Compensated.
    Compensated,
    /// Failed.
    Failed,
    /// Completed.
    Completed,
}

impl ProcessStatus {
    /// Borrow as r.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Retrying => "retrying",
            Self::Compensating => "compensating",
            Self::Compensated => "compensated",
            Self::Failed => "failed",
            Self::Completed => "completed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Process state structure.
pub struct ProcessState {
    /// Row identifier.
    pub id: Uuid,
    /// Workflow type.
    pub workflow_type: String,
    /// Correlation key.
    pub correlation_key: String,
    /// State.
    pub state: Value,
    /// Status.
    pub status: ProcessStatus,
    /// Completed steps.
    pub completed_steps: Vec<String>,
    /// Version.
    pub version: i64,
    /// Attempts.
    pub attempts: i32,
    /// Next attempt at.
    pub next_attempt_at: Option<DateTime<Utc>>,
    /// Last error.
    pub last_error: Option<Value>,
    /// Created at.
    pub created_at: DateTime<Utc>,
    /// Updated at.
    pub updated_at: DateTime<Utc>,
}

impl ProcessState {
    /// Construct a new value.
    pub fn new(
        workflow_type: impl Into<String>,
        correlation_key: impl Into<String>,
        state: Value,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            workflow_type: workflow_type.into(),
            correlation_key: correlation_key.into(),
            state,
            status: ProcessStatus::Pending,
            completed_steps: Vec::new(),
            version: 0,
            attempts: 0,
            next_attempt_at: Some(now),
            last_error: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Step completed.
    pub fn step_completed(&self, step: &str) -> bool {
        self.completed_steps
            .iter()
            .any(|candidate| candidate == step)
    }
}

/// Durable process-manager persistence with optimistic concurrency and due-work leasing.
#[async_trait]
pub trait ProcessManagerStore: Send + Sync {
    /// Create.
    async fn create(
        &self,
        _uow: Option<&crate::datastore::CommandUnitOfWork>,
        process: ProcessState,
    ) -> RiverbaseResult<ProcessState>;
    /// Load.
    async fn load(&self, id: Uuid) -> RiverbaseResult<Option<ProcessState>>;
    /// Load by correlation.
    async fn load_by_correlation(
        &self,
        workflow_type: &str,
        correlation_key: &str,
    ) -> RiverbaseResult<Option<ProcessState>>;
    /// Save.
    async fn save(
        &self,
        _uow: Option<&crate::datastore::CommandUnitOfWork>,
        process: ProcessState,
        expected_version: i64,
    ) -> RiverbaseResult<ProcessState>;
    /// Claim due.
    async fn claim_due(&self, limit: i64) -> RiverbaseResult<Vec<ProcessState>>;
}

/// Process step idempotency key.
pub fn process_step_idempotency_key(
    workflow_id: Uuid,
    step_name: &str,
    target_domain: &str,
) -> String {
    format!("workflow:{workflow_id}:{step_name}:{target_domain}")
}
