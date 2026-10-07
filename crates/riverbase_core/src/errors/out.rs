//! `OUT` error catalogue.

crate::declare_errors! {
    type_base: "https://riverbase.io/~/rs/error/",
    OUT_001 {
        status: 404,
        title: "Not Found",
        code: "OUT-001",
        message: "Outbox record was not found.",
    },
    OUT_002 {
        status: 500,
        title: "Internal Server Error",
        code: "OUT-002",
        message: "Failed to enqueue outbox record.",
    },
    OUT_003 {
        status: 500,
        title: "Internal Server Error",
        code: "OUT-003",
        message: "Failed to claim outbox records.",
    },
    OUT_004 {
        status: 500,
        title: "Internal Server Error",
        code: "OUT-004",
        message: "Failed to mark outbox record published.",
    },
    OUT_005 {
        status: 500,
        title: "Internal Server Error",
        code: "OUT-005",
        message: "Failed to reschedule outbox record.",
    },
}
