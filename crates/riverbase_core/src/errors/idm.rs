//! `IDM` error catalogue.

crate::declare_errors! {
    type_base: "https://riverbase.io/~/rs/error/",
    IDM_001 {
        status: 500,
        title: "Internal Server Error",
        code: "IDM-001",
        message: "Failed to claim idempotency key.",
    },
    IDM_002 {
        status: 500,
        title: "Internal Server Error",
        code: "IDM-002",
        message: "Failed to load idempotency key.",
    },
    IDM_003 {
        status: 500,
        title: "Internal Server Error",
        code: "IDM-003",
        message: "Failed to complete idempotency key.",
    },
    IDM_004 {
        status: 500,
        title: "Internal Server Error",
        code: "IDM-004",
        message: "Failed to persist failed idempotency attempt.",
    },
    IDM_005 {
        status: 409,
        title: "Conflict",
        code: "IDM-005",
        message: "Idempotency claim is no longer owned by this command.",
    },
    IDM_006 {
        status: 500,
        title: "Internal Server Error",
        code: "IDM-006",
        message: "Failed to reclaim stale idempotency key.",
    },
}
