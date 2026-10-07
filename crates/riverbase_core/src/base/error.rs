use std::collections::HashSet;
use std::fmt;
use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use serde_json::{json, Value};

/// Riverbase result type alias.
pub type RiverbaseResult<T> = Result<T, RiverbaseError>;

/// RFC 7807 problem ``type`` URI prefix (framework default).
pub const ERROR_TYPE_BASE: &str = "https://riverbase.io/~/rs/error/";

/// Library-wide error code: `{MODULE}-{SERIAL}` (3-character module + 3-digit serial), e.g. `C00-001`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct RiverbaseErrorCode {
    code: String,
    #[serde(skip)]
    type_base: &'static str,
}

impl RiverbaseErrorCode {
    /// Construct a new value.
    pub fn new(code: &str) -> Self {
        if let Err(reason) = Self::validate(code) {
            tracing::error!(
                invalid_error_code = code,
                reason,
                "invalid Riverbase error code replaced with C00-000"
            );
            return Self {
                code: "C00-000".to_string(),
                type_base: ERROR_TYPE_BASE,
            };
        }
        let registry = REGISTERED_ERROR_CODES.get_or_init(|| Mutex::new(HashSet::new()));
        if let Ok(mut set) = registry.lock() {
            if !set.insert(code.to_string()) {
                // Re-registering the same code from multiple call sites is expected.
            }
        }
        Self {
            code: code.to_string(),
            type_base: ERROR_TYPE_BASE,
        }
    }

    /// Wrap a catalogue literal with the framework type base. No validation or registry insert.
    pub fn from_static(code: &'static str) -> Self {
        Self::from_static_with_base(code, ERROR_TYPE_BASE)
    }

    /// Wrap a catalogue literal with a crate-specific type base. No validation or registry insert.
    pub fn from_static_with_base(code: &'static str, type_base: &'static str) -> Self {
        Self {
            code: code.to_string(),
            type_base,
        }
    }

    /// RFC 7807 ``type`` URI prefix carried with this code.
    pub fn type_base(&self) -> &'static str {
        self.type_base
    }

    /// Validate.
    pub fn validate(code: &str) -> Result<(), &'static str> {
        let bytes = code.as_bytes();
        if bytes.len() != 7 || bytes.get(3) != Some(&b'-') {
            return Err("expected XXX-XXX");
        }
        if !bytes[..3].iter().all(u8::is_ascii_alphanumeric) {
            return Err("module must match [A-Za-z0-9]{3}");
        }
        if !bytes[4..].iter().all(u8::is_ascii_digit) {
            return Err("serial must contain three digits");
        }
        Ok(())
    }

    /// Borrow as r.
    pub fn as_str(&self) -> &str {
        &self.code
    }
}

impl fmt::Display for RiverbaseErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.code)
    }
}

static REGISTERED_ERROR_CODES: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

/// Structured error returned across flrs crates.
#[derive(Clone, Serialize)]
pub struct RiverbaseError {
    /// Http status.
    pub http_status: u16,
    /// Http title.
    pub http_title: String,
    /// User-facing message.
    pub errmesg: String,
    /// Unique library-wide code (`XXX-NNN`).
    pub errcode: RiverbaseErrorCode,
    /// Developer-oriented details (context, inner cause, identifiers, …).
    pub errdata: Value,
    /// Optional user-facing hint for resolving the error (mirrors Python `errhint`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errhint: Option<String>,
}

impl RiverbaseError {
    /// Construct a new value.
    pub fn new(
        http_status: u16,
        http_title: impl Into<String>,
        errcode: RiverbaseErrorCode,
        errmesg: impl Into<String>,
        errdata: impl IntoErrorData,
    ) -> Self {
        Self {
            http_status,
            http_title: http_title.into(),
            errmesg: errmesg.into(),
            errcode,
            errdata: errdata.into_error_data(),
            errhint: None,
        }
    }

    /// RFC 7807 ``type`` URI prefix (`{base}{errcode}`).
    pub fn error_type_base(&self) -> &'static str {
        self.errcode.type_base()
    }

    /// Attach a user-facing remediation hint (serialized as `errhint` in ProblemDetails).
    pub fn with_errhint(mut self, errhint: impl Into<String>) -> Self {
        self.errhint = Some(errhint.into());
        self
    }

    /// Emit a structured tracing event (same shape as other runtime logs).
    pub fn log(&self) {
        tracing::error!(
            errcode = %self.errcode,
            http_status = self.http_status,
            http_title = %self.http_title,
            errdata = %self.errdata,
            errhint = ?self.errhint,
            "{}",
            self.errmesg
        );
    }
}

/// Not found error; structure.
pub struct NotFoundError;
/// Invalid argument error; structure.
pub struct InvalidArgumentError;
/// Bad request error type alias.
pub type BadRequestError = InvalidArgumentError;
/// Forbidden error; structure.
pub struct ForbiddenError;
/// Conflict error; structure.
pub struct ConflictError;
/// Storage error; structure.
pub struct StorageError;
/// Config error; structure.
pub struct ConfigError;

impl NotFoundError {
    /// With.
    pub fn with(
        errcode: &str,
        errmesg: impl Into<String>,
        errdata: impl IntoErrorData,
    ) -> RiverbaseError {
        RiverbaseError::new(
            404,
            "Not Found",
            RiverbaseErrorCode::new(errcode),
            errmesg,
            errdata,
        )
    }
}

impl InvalidArgumentError {
    /// With.
    pub fn with(
        errcode: &str,
        errmesg: impl Into<String>,
        errdata: impl IntoErrorData,
    ) -> RiverbaseError {
        RiverbaseError::new(
            422,
            "Unprocessable Content",
            RiverbaseErrorCode::new(errcode),
            errmesg,
            errdata,
        )
    }
}

impl ConflictError {
    /// With.
    pub fn with(
        errcode: &str,
        errmesg: impl Into<String>,
        errdata: impl IntoErrorData,
    ) -> RiverbaseError {
        RiverbaseError::new(
            409,
            "Conflict",
            RiverbaseErrorCode::new(errcode),
            errmesg,
            errdata,
        )
    }
}

impl ForbiddenError {
    /// With.
    pub fn with(
        errcode: &str,
        errmesg: impl Into<String>,
        errdata: impl IntoErrorData,
    ) -> RiverbaseError {
        RiverbaseError::new(
            403,
            "Forbidden",
            RiverbaseErrorCode::new(errcode),
            errmesg,
            errdata,
        )
    }
}

impl StorageError {
    /// With.
    pub fn with(
        errcode: &str,
        errmesg: impl Into<String>,
        errdata: impl IntoErrorData,
    ) -> RiverbaseError {
        RiverbaseError::new(
            500,
            "Internal Server Error",
            RiverbaseErrorCode::new(errcode),
            errmesg,
            errdata,
        )
    }

    /// Log the driver/pool error and return stable, non-revealing [`RiverbaseError::errdata`].
    ///
    /// Use this on Diesel, connection-pool, and filesystem driver failures so table names,
    /// SQL fragments, and DSNs do not reach the HTTP problem body ([SUR-10]).
    pub fn from_driver(
        errcode: &str,
        errmesg: impl Into<String>,
        err: impl std::fmt::Display + std::fmt::Debug,
    ) -> RiverbaseError {
        tracing::error!(
            errcode,
            error = %err,
            detail = ?err,
            "storage driver error"
        );
        Self::with(errcode, errmesg, json!({ "kind": "driver" }))
    }
}

impl ConfigError {
    /// With.
    pub fn with(
        errcode: &str,
        errmesg: impl Into<String>,
        errdata: impl IntoErrorData,
    ) -> RiverbaseError {
        RiverbaseError::new(
            500,
            "Internal Server Error",
            RiverbaseErrorCode::new(errcode),
            errmesg,
            errdata,
        )
    }

    /// Operator-facing config violation: empty `errdata`, remediation in `errhint`.
    pub fn with_hint(
        errcode: &str,
        errmesg: impl Into<String>,
        errhint: impl Into<String>,
    ) -> RiverbaseError {
        RiverbaseError::new(
            500,
            "Internal Server Error",
            RiverbaseErrorCode::new(errcode),
            errmesg,
            Value::Null,
        )
        .with_errhint(errhint)
    }
}

impl fmt::Display for RiverbaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.errmesg, self.errcode)
    }
}

impl fmt::Debug for RiverbaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Prefer a single-line shape over the derived struct dump so process-fatal
        // `Error: {:?}` output stays readable when mains return `Result`.
        write!(
            f,
            "{} ({}) http_status={} errdata={} errhint={:?}",
            self.errmesg, self.errcode, self.http_status, self.errdata, self.errhint
        )
    }
}

impl std::error::Error for RiverbaseError {}

/// Into error data trait.
pub trait IntoErrorData {
    /// Convert into error data.
    fn into_error_data(self) -> Value;
}

impl IntoErrorData for Value {
    fn into_error_data(self) -> Value {
        self
    }
}

impl IntoErrorData for &str {
    fn into_error_data(self) -> Value {
        json!({ "detail": self })
    }
}

impl IntoErrorData for String {
    fn into_error_data(self) -> Value {
        json!({ "detail": self })
    }
}

impl<T: Serialize> IntoErrorData for &T {
    fn into_error_data(self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|e| json!({ "detail": e.to_string() }))
    }
}

/// Static catalogue entry: status, title, code, message, optional hint, and RFC 7807 type base.
///
/// Raise sites only attach [`errdata`](RiverbaseError::errdata). A catalogue [`errhint`]
/// is applied unless the call site replaces it with [`with_hint`](ErrorSpec::with_hint)
/// or [`RiverbaseError::with_errhint`].
/// Catalogue literals are trusted — no grammar check, `C00-000` fallback, or registry insert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorSpec {
    /// RFC 7807 ``type`` URI prefix (trailing slash), e.g. `https://riverbase.io/~/rs/error/`.
    pub error_type_base: &'static str,
    /// HTTP status.
    pub http_status: u16,
    /// HTTP title.
    pub http_title: &'static str,
    /// Unique library-wide code (`XXX-NNN`).
    pub errcode: &'static str,
    /// User-facing message.
    pub errmesg: &'static str,
    /// Default remediation hint; omitted on the wire when `None`.
    pub errhint: Option<&'static str>,
}

impl ErrorSpec {
    /// Raise with developer [`errdata`](RiverbaseError::errdata) and the catalogue default hint.
    pub fn with_data(self, errdata: impl IntoErrorData) -> RiverbaseError {
        let err = RiverbaseError::new(
            self.http_status,
            self.http_title,
            RiverbaseErrorCode::from_static_with_base(self.errcode, self.error_type_base),
            self.errmesg,
            errdata,
        );
        match self.errhint {
            Some(hint) => err.with_errhint(hint),
            None => err,
        }
    }

    /// Raise with empty `errdata` and the catalogue default hint (if any).
    pub fn raise(self) -> RiverbaseError {
        self.with_data(Value::Null)
    }

    /// Raise with empty `errdata`, replacing any catalogue default hint.
    pub fn with_hint(self, errhint: impl Into<String>) -> RiverbaseError {
        RiverbaseError::new(
            self.http_status,
            self.http_title,
            RiverbaseErrorCode::from_static_with_base(self.errcode, self.error_type_base),
            self.errmesg,
            Value::Null,
        )
        .with_errhint(errhint)
    }

    /// Log a driver/pool failure and raise with stable [`RiverbaseError::errdata`] `{ "kind": "driver" }`.
    pub fn from_driver(self, err: impl std::fmt::Display + std::fmt::Debug) -> RiverbaseError {
        tracing::error!(
            errcode = self.errcode,
            error = %err,
            detail = ?err,
            "storage driver error"
        );
        self.with_data(json!({ "kind": "driver" }))
    }
}

/// Declare a crate error catalogue. `type_base` applies to every spec. No validation.
/// Each `errcode` and each `message` must be unique. Optional `hint` is the default
/// `errhint` unless the raise site replaces it. Call `with_data` / `raise` at the site
/// — do not add a function that only forwards to a spec.
///
/// ```ignore
/// riverbase_core::declare_errors! {
///     type_base: "https://riverbase.io/~/rs/error/",
///     IDM_029 {
///         status: 400,
///         title: "Bad Request",
///         code: "IDM-029",
///         message: "Active profile is invalid or not owned by the authenticated user.",
///         hint: "Choose a profile owned by the signed-in user.",
///     }
/// }
/// ```
#[macro_export]
macro_rules! declare_errors {
    (
        type_base: $base:expr,
        $(
            $name:ident {
                status: $status:expr,
                title: $title:expr,
                code: $code:expr,
                message: $message:expr
                $(, hint: $hint:expr)?
                $(,)?
            }
        ),* $(,)?
    ) => {
        $(
            /// Predefined error spec.
            pub const $name: $crate::base::ErrorSpec = $crate::base::ErrorSpec {
                error_type_base: $base,
                http_status: $status,
                http_title: $title,
                errcode: $code,
                errmesg: $message,
                errhint: $crate::declare_errors!(@hint $($hint)?),
            };
        )*
    };
    (@hint) => {
        None
    };
    (@hint $hint:expr) => {
        Some($hint)
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::ProblemDetails;

    crate::declare_errors! {
        type_base: "https://entry.express/~/rs/error/",
        SAMPLE_391 {
            status: 400,
            title: "Bad Request",
            code: "XYZ-391",
            message: "Sample catalogue error.",
        },
        SAMPLE_392 {
            status: 400,
            title: "Bad Request",
            code: "XYZ-392",
            message: "Sample catalogue error with a default hint.",
            hint: "Check the identifier and retry.",
        }
    }

    #[test]
    fn spec_raise_preserves_metadata_and_custom_type_base() {
        let err = SAMPLE_391.with_data(json!({ "id": "1" }));
        assert_eq!(err.http_status, 400);
        assert_eq!(err.http_title, "Bad Request");
        assert_eq!(err.errcode.as_str(), "XYZ-391");
        assert_eq!(err.errmesg, "Sample catalogue error.");
        assert_eq!(err.errdata, json!({ "id": "1" }));
        assert_eq!(err.errhint, None);
        assert_eq!(err.error_type_base(), "https://entry.express/~/rs/error/");
        let problem = ProblemDetails::from_riverbase_error(&err, None);
        assert_eq!(
            problem.problem_type,
            "https://entry.express/~/rs/error/XYZ-391"
        );
    }

    #[test]
    fn spec_with_hint_sets_errhint() {
        let err = SAMPLE_391.with_hint("Try again.");
        assert_eq!(err.errhint.as_deref(), Some("Try again."));
        assert_eq!(err.errdata, Value::Null);
    }

    #[test]
    fn spec_default_hint_applies_on_with_data() {
        let err = SAMPLE_392.with_data(json!({ "id": "1" }));
        assert_eq!(
            err.errhint.as_deref(),
            Some("Check the identifier and retry.")
        );
        assert_eq!(err.errdata, json!({ "id": "1" }));
        assert_eq!(
            SAMPLE_392.raise().errhint.as_deref(),
            Some("Check the identifier and retry.")
        );
    }

    #[test]
    fn spec_call_site_replaces_default_hint() {
        let err = SAMPLE_392.with_hint("Use a different profile.");
        assert_eq!(err.errhint.as_deref(), Some("Use a different profile."));
        let err = SAMPLE_392
            .with_data(json!({ "id": "1" }))
            .with_errhint("Use a different profile.");
        assert_eq!(err.errhint.as_deref(), Some("Use a different profile."));
    }

    #[test]
    fn ad_hoc_new_uses_default_type_base() {
        let err = RiverbaseError::new(
            404,
            "Not Found",
            RiverbaseErrorCode::new("DAT-001"),
            "Resource was not found.",
            json!(null),
        );
        assert_eq!(err.error_type_base(), ERROR_TYPE_BASE);
        let problem = ProblemDetails::from_riverbase_error(&err, None);
        assert_eq!(problem.problem_type, format!("{ERROR_TYPE_BASE}DAT-001"));
    }

    #[test]
    fn from_static_skips_validation() {
        let code = RiverbaseErrorCode::from_static("not-valid");
        assert_eq!(code.as_str(), "not-valid");
    }
}
