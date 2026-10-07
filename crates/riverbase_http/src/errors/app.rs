//! `APP` error catalogue.

riverbase_core::declare_errors! {
    type_base: "https://riverbase.io/~/rs/error/",
    APP_001 {
        status: 500,
        title: "Internal Server Error",
        code: "APP-001",
        message: "Operation failed (APP-001).",
    },
    APP_002 {
        status: 422,
        title: "Unprocessable Content",
        code: "APP-002",
        message: "Portal runtime not initialized.",
    },
    APP_003 {
        status: 422,
        title: "Unprocessable Content",
        code: "APP-003",
        message: "Unsupported persistence backend for portal domains.",
    },
    APP_004 {
        status: 500,
        title: "Internal Server Error",
        code: "APP-004",
        message: "An application module was registered more than once.",
    },
    APP_020 {
        status: 500,
        title: "Internal Server Error",
        code: "APP-020",
        message: "Mounted domain exposes a query resource with an empty name.",
    },
    APP_021 {
        status: 500,
        title: "Internal Server Error",
        code: "APP-021",
        message: "Mounted domain query route meta has an empty resource name.",
    },
    APP_022 {
        status: 500,
        title: "Internal Server Error",
        code: "APP-022",
        message: "Portal initialization did not produce a domain runtime.",
    },
    APP_023 {
        status: 500,
        title: "Internal Server Error",
        code: "APP-023",
        message: "A domain namespace was mounted more than once.",
    },
}
