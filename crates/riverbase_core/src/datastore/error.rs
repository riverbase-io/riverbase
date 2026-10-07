use crate::base::{RiverbaseError, IntoErrorData, InvalidArgumentError, NotFoundError, StorageError};

/// Data result type alias.
pub type DataResult<T> = crate::base::RiverbaseResult<T>;

/// Namespace for constructing datastore errors with explicit `XXX-NNN` codes.
///
/// Construct with [`DataError::not_found`], [`DataError::storage`],
/// [`DataError::unsupported`], or [`DataError::conflict`] at the detection site.
pub struct DataError;

impl DataError {
    /// Not found.
    pub fn not_found(code: &str, msg: impl Into<String>, data: impl IntoErrorData) -> RiverbaseError {
        NotFoundError::with(code, msg, data)
    }

    /// Storage.
    pub fn storage(code: &str, msg: impl Into<String>, data: impl IntoErrorData) -> RiverbaseError {
        StorageError::with(code, msg, data)
    }

    /// Diesel / pool failure: log the driver text and put a stable summary in `errdata`.
    pub fn driver(
        code: &str,
        msg: impl Into<String>,
        err: impl std::fmt::Display + std::fmt::Debug,
    ) -> RiverbaseError {
        StorageError::from_driver(code, msg, err)
    }

    /// Unsupported.
    pub fn unsupported(
        code: &str,
        msg: impl Into<String>,
        data: impl IntoErrorData,
    ) -> RiverbaseError {
        InvalidArgumentError::with(code, msg, data)
    }

    /// Precondition / If-Match failure (HTTP 412).
    pub fn conflict(code: &str, msg: impl Into<String>, data: impl IntoErrorData) -> RiverbaseError {
        RiverbaseError::new(
            412,
            "Precondition Failed",
            crate::base::RiverbaseErrorCode::new(code),
            msg,
            data,
        )
    }
}

/// Whether this is not found.
pub fn is_not_found(err: &RiverbaseError) -> bool {
    err.http_status == 404
}

impl From<diesel::result::Error> for RiverbaseError {
    fn from(value: diesel::result::Error) -> Self {
        match value {
            diesel::result::Error::NotFound => crate::errors::DAT_001.with_data("diesel NotFound"),
            diesel::result::Error::DatabaseError(kind, info) => {
                tracing::error!(
                    error = info.message(),
                    diesel_kind = ?kind,
                    constraint = info.constraint_name().unwrap_or(""),
                    table = info.table_name().unwrap_or(""),
                    column = info.column_name().unwrap_or(""),
                    "diesel database error"
                );
                // Client `errdata` is the kind name only. Constraint, table, and
                // column names stay in the log above.
                let errdata = serde_json::json!({ "kind": format!("{kind:?}") });
                match kind {
                    diesel::result::DatabaseErrorKind::UniqueViolation => {
                        crate::errors::DAT_090.with_data(errdata)
                    }
                    diesel::result::DatabaseErrorKind::ForeignKeyViolation => {
                        crate::errors::DAT_091.with_data(errdata)
                    }
                    diesel::result::DatabaseErrorKind::NotNullViolation => {
                        crate::errors::DAT_092.with_data(errdata)
                    }
                    diesel::result::DatabaseErrorKind::CheckViolation => {
                        crate::errors::DAT_093.with_data(errdata)
                    }
                    diesel::result::DatabaseErrorKind::SerializationFailure => {
                        crate::errors::DAT_095.with_data(errdata)
                    }
                    diesel::result::DatabaseErrorKind::ReadOnlyTransaction => {
                        crate::errors::DAT_096.with_data(errdata)
                    }
                    diesel::result::DatabaseErrorKind::ClosedConnection => {
                        crate::errors::DAT_097.with_data(errdata)
                    }
                    diesel::result::DatabaseErrorKind::UnableToSendCommand => {
                        crate::errors::DAT_098.with_data(errdata)
                    }
                    // diesel 2.2 has no `ExclusionViolation` (SQLSTATE 23P01).
                    // Those failures arrive as `Unknown` and stay `DAT-002`.
                    // `DAT-094` is reserved for that kind.
                    diesel::result::DatabaseErrorKind::Unknown => {
                        crate::errors::DAT_002.with_data(errdata)
                    }
                    _ => crate::errors::DAT_002.with_data(errdata),
                }
            }
            other => {
                let kind = match &other {
                    diesel::result::Error::QueryBuilderError(_) => "query_builder".to_string(),
                    diesel::result::Error::DeserializationError(_) => "deserialize".to_string(),
                    diesel::result::Error::SerializationError(_) => "serialize".to_string(),
                    diesel::result::Error::RollbackTransaction => "rollback".to_string(),
                    diesel::result::Error::AlreadyInTransaction => "in_transaction".to_string(),
                    diesel::result::Error::NotInTransaction => "not_in_transaction".to_string(),
                    diesel::result::Error::BrokenTransactionManager => {
                        "broken_transaction".to_string()
                    }
                    _ => "driver".to_string(),
                };
                tracing::error!(
                    error = %other,
                    diesel_kind = %kind,
                    "diesel driver error"
                );
                crate::errors::DAT_002.with_data(serde_json::json!({ "kind": kind }))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use diesel::result::{DatabaseErrorInformation, DatabaseErrorKind};

    use crate::base::RiverbaseError;

    struct LeakyDbError;

    impl DatabaseErrorInformation for LeakyDbError {
        fn message(&self) -> &str {
            "duplicate key value violates unique constraint \"users_email_key\""
        }
        fn details(&self) -> Option<&str> {
            Some("Key (password)=(secret) already exists.")
        }
        fn hint(&self) -> Option<&str> {
            None
        }
        fn table_name(&self) -> Option<&str> {
            Some("secret_table")
        }
        fn column_name(&self) -> Option<&str> {
            Some("password")
        }
        fn constraint_name(&self) -> Option<&str> {
            Some("users_email_key")
        }
        fn statement_position(&self) -> Option<i32> {
            None
        }
    }

    #[test]
    fn diesel_schema_error_errdata_omits_sql_identifiers() {
        let leaked = "column secret_table.password of relation public.users does not exist";
        let err: RiverbaseError = diesel::result::Error::QueryBuilderError(Box::new(
            std::io::Error::new(std::io::ErrorKind::InvalidInput, leaked),
        ))
        .into();
        let body = err.errdata.to_string();
        assert!(
            !body.contains("secret_table") && !body.contains("password") && !body.contains("users"),
            "errdata leaked driver text: {body}"
        );
        assert_eq!(err.errdata["kind"], "query_builder");
        assert_eq!(err.errcode.as_str(), "DAT-002");
        assert_eq!(err.http_status, 500);
    }

    #[test]
    fn diesel_unique_violation_is_conflict_without_sql_identifiers() {
        let err: RiverbaseError = diesel::result::Error::DatabaseError(
            DatabaseErrorKind::UniqueViolation,
            Box::new(LeakyDbError),
        )
        .into();
        let body = err.errdata.to_string();
        assert!(
            !body.contains("secret_table")
                && !body.contains("password")
                && !body.contains("users_email_key")
                && !body.contains("users"),
            "errdata leaked driver text: {body}"
        );
        assert_eq!(err.errdata["kind"], "UniqueViolation");
        assert_eq!(err.errcode.as_str(), "DAT-090");
        assert_eq!(err.http_status, 409);
        assert_eq!(err.http_title, "Conflict");
        assert_eq!(err.errmesg, "A record with these values already exists.");
        assert_eq!(
            err.errhint.as_deref(),
            Some("Use a different value for the field that must be unique.")
        );
    }

    #[test]
    fn diesel_exclusion_violation_code_is_reserved() {
        // diesel 2.2 has no `DatabaseErrorKind::ExclusionViolation`, so the
        // translator cannot raise this yet. The spec stays allocated.
        let spec = crate::errors::DAT_094;
        assert_eq!(spec.errcode, "DAT-094");
        assert_eq!(spec.http_status, 409);
        assert_eq!(
            spec.errmesg,
            "The record overlaps an existing exclusion constraint."
        );
    }

    #[test]
    fn diesel_database_kinds_use_specific_codes() {
        let cases = [
            (DatabaseErrorKind::ForeignKeyViolation, "DAT-091", 409),
            (DatabaseErrorKind::NotNullViolation, "DAT-092", 422),
            (DatabaseErrorKind::CheckViolation, "DAT-093", 422),
            (DatabaseErrorKind::SerializationFailure, "DAT-095", 409),
            (DatabaseErrorKind::ReadOnlyTransaction, "DAT-096", 500),
            (DatabaseErrorKind::ClosedConnection, "DAT-097", 500),
            (DatabaseErrorKind::UnableToSendCommand, "DAT-098", 500),
            (DatabaseErrorKind::Unknown, "DAT-002", 500),
        ];
        for (kind, code, status) in cases {
            let err: RiverbaseError = diesel::result::Error::DatabaseError(
                kind,
                Box::new(format!("secret_table.{kind:?}")),
            )
            .into();
            assert_eq!(err.errcode.as_str(), code, "{kind:?}");
            assert_eq!(err.http_status, status, "{kind:?}");
            assert_eq!(err.errdata["kind"], format!("{kind:?}"), "{kind:?}");
            assert!(
                !err.errdata.to_string().contains("secret_table"),
                "{kind:?} leaked driver text"
            );
        }
    }
}
