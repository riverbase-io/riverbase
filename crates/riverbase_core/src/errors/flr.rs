//! `FLR` error catalogue.

crate::declare_errors! {
    type_base: "https://riverbase.io/~/rs/error/",
    FLR_001 {
        status: 500,
        title: "Internal Server Error",
        code: "FLR-001",
        message: "Failed to connect to Postgres.",
    },
    FLR_010 {
        status: 404,
        title: "Not Found",
        code: "FLR-010",
        message: "No command engine is registered for this namespace.",
    },
    FLR_011 {
        status: 409,
        title: "Conflict",
        code: "FLR-011",
        message: "Maximum nested command invocation depth was exceeded.",
    },
    FLR_012 {
        status: 403,
        title: "Forbidden",
        code: "FLR-012",
        message: "Service command invocation requires an explicit capability.",
    },
    FLR_013 {
        status: 404,
        title: "Not Found",
        code: "FLR-013",
        message: "No query engine is registered for this namespace.",
    },
}
