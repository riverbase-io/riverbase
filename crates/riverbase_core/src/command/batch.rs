//! Internal multi-command unit-of-work (one host transaction, no public HTTP batch API).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::command::CommandTarget;

/// A host command already planned for execution inside a batch UoW.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedCommand {
    /// Registered command key.
    pub cmdkey: String,
    /// Command payload.
    pub payload: Value,
    /// Dispatch target (object or collection).
    pub target: CommandTarget,
}

/// Per-item status inside a prepared batch result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchItemStatus {
    /// Command committed as part of the batch transaction.
    Ok,
    /// Command failed; the batch transaction was rolled back.
    Failed,
    /// Not executed because an earlier item failed.
    NotRun,
}

/// Outcome for one item in a prepared batch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchItemOutcome {
    /// Zero-based index in the submitted `items` slice.
    pub index: usize,
    /// Command key for this item.
    pub cmdkey: String,
    /// Status recorded once, right when this item's own attempt concludes: `ok` / `failed`
    /// from actually running it, or `not_run` when skipped because an earlier item already
    /// failed. Never rewritten afterward — this is what happened to this item's own attempt,
    /// independent of whether the batch transaction that wrapped it ultimately committed.
    pub immediate_status: BatchItemStatus,
    /// Status recorded once, when the batch as a whole concludes. Equals `immediate_status`
    /// when the batch committed; forced to `failed` for every attempted (non-`not_run`) item
    /// when the batch rolled back, since nothing it did was persisted.
    pub final_status: BatchItemStatus,
    /// Handler response when `immediate_status == ok`.
    pub result: Option<Value>,
    /// Structured error when `immediate_status == failed`.
    pub error: Option<Value>,
}

/// Result of executing a prepared command batch in one unit of work.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchExecuteResult {
    /// `true` when every item's `final_status` is `ok` (i.e. the host transaction committed).
    pub ok: bool,
    /// One outcome per submitted item (including `not_run` tail items on failure).
    pub items: Vec<BatchItemOutcome>,
}

impl BatchExecuteResult {
    /// Construct a result from outcomes whose `final_status` has already been decided.
    /// `ok` is derived from `final_status` so it can never drift from the per-item record.
    pub fn from_outcomes(items: Vec<BatchItemOutcome>) -> Self {
        let ok = items
            .iter()
            .all(|item| item.final_status == BatchItemStatus::Ok);
        Self { ok, items }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(status: BatchItemStatus) -> BatchItemOutcome {
        BatchItemOutcome {
            index: 0,
            cmdkey: "test".into(),
            immediate_status: status,
            final_status: status,
            result: None,
            error: None,
        }
    }

    #[test]
    fn from_outcomes_is_ok_only_when_every_final_status_is_ok() {
        let all_ok = BatchExecuteResult::from_outcomes(vec![
            outcome(BatchItemStatus::Ok),
            outcome(BatchItemStatus::Ok),
        ]);
        assert!(all_ok.ok);

        let one_failed = BatchExecuteResult::from_outcomes(vec![
            outcome(BatchItemStatus::Ok),
            outcome(BatchItemStatus::Failed),
        ]);
        assert!(!one_failed.ok);

        let empty = BatchExecuteResult::from_outcomes(Vec::new());
        assert!(empty.ok, "an empty batch is vacuously ok");
    }
}
