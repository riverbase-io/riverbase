//! `WEB` error catalogue.

crate::declare_errors! {
    type_base: "https://riverbase.io/~/rs/error/",
    WEB_001 {
        status: 422,
        title: "Unprocessable Content",
        code: "WEB-001",
        message: "HTTP request validation failed.",
    },
    WEB_105 {
        status: 422,
        title: "Unprocessable Content",
        code: "WEB-105",
        message: "Query statement must be a JSON object.",
    },
    WEB_106 {
        status: 422,
        title: "Unprocessable Content",
        code: "WEB-106",
        message: "Composite group operand must be a list of statements.",
    },
}
