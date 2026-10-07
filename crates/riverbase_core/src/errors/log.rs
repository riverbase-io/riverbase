//! `LOG` error catalogue.

crate::declare_errors! {
    type_base: "https://riverbase.io/~/rs/error/",
    LOG_002 {
        status: 404,
        title: "Not Found",
        code: "LOG-002",
        message: "Response log is disabled.",
    },
    LOG_004 {
        status: 500,
        title: "Internal Server Error",
        code: "LOG-004",
        message: "Failed to insert command log record.",
    },
    LOG_007 {
        status: 500,
        title: "Internal Server Error",
        code: "LOG-007",
        message: "Failed to insert event log record.",
    },
    LOG_010 {
        status: 500,
        title: "Internal Server Error",
        code: "LOG-010",
        message: "Failed to insert message log record.",
    },
    LOG_013 {
        status: 500,
        title: "Internal Server Error",
        code: "LOG-013",
        message: "Failed to insert activity log record.",
    },
    LOG_015 {
        status: 404,
        title: "Not Found",
        code: "LOG-015",
        message: "Command response was not found.",
    },
    LOG_016 {
        status: 500,
        title: "Internal Server Error",
        code: "LOG-016",
        message: "Failed to insert query log record.",
    },
    LOG_017 {
        status: 404,
        title: "Not Found",
        code: "LOG-017",
        message: "Command log record was not found.",
    },
    LOG_018 {
        status: 500,
        title: "Internal Server Error",
        code: "LOG-018",
        message: "Failed to update command log status.",
    },
    LOG_019 {
        status: 500,
        title: "Internal Server Error",
        code: "LOG-019",
        message: "Failed to persist command response.",
    },
    LOG_020 {
        status: 500,
        title: "Internal Server Error",
        code: "LOG-020",
        message: "Failed to load command response.",
    },
    LOG_021 {
        status: 500,
        title: "Internal Server Error",
        code: "LOG-021",
        message: "Failed to insert context log record.",
    },
}
