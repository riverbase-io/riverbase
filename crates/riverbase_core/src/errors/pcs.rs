//! `PCS` error catalogue.

crate::declare_errors! {
    type_base: "https://riverbase.io/~/rs/error/",
    PCS_001 {
        status: 409,
        title: "Conflict",
        code: "PCS-001",
        message: "Process correlation key already exists.",
    },
    PCS_003 {
        status: 409,
        title: "Conflict",
        code: "PCS-003",
        message: "Process manager version is stale.",
    },
    PCS_004 {
        status: 500,
        title: "Internal Server Error",
        code: "PCS-004",
        message: "Process manager has an invalid status.",
    },
    PCS_005 {
        status: 500,
        title: "Internal Server Error",
        code: "PCS-005",
        message: "Process manager persistence failed.",
    },
}
