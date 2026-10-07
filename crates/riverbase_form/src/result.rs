//! Result alias and leftover `FormError` constructors for `riverbase_form`.
//!
//! Raise catalogue codes from [`crate::errors`]. `FormError` remains for callers
//! that still construct `RiverbaseError` by hand.

use riverbase_core::base::{RiverbaseError, InvalidArgumentError, NotFoundError, StorageError};
use serde_json::Value;

/// Result alias for form operations.
pub type FormResult<T> = Result<T, RiverbaseError>;

/// General form errors (HTTP 400).
pub struct FormError;

impl FormError {
    pub fn with(code: &str, msg: impl Into<String>) -> RiverbaseError {
        InvalidArgumentError::with(code, msg, Value::Null)
    }

    pub fn with_detail(
        code: &str,
        msg: impl Into<String>,
        detail: impl Into<String>,
    ) -> RiverbaseError {
        InvalidArgumentError::with(code, msg, detail.into())
    }

    pub fn not_found(
        code: &str,
        msg: impl Into<String>,
        detail: impl Into<String>,
    ) -> RiverbaseError {
        NotFoundError::with(code, msg, detail.into())
    }

    pub fn storage(code: &str, msg: impl Into<String>, detail: impl Into<String>) -> RiverbaseError {
        StorageError::with(code, msg, detail.into())
    }
}
