//! `RXD` error catalogue.

riverbase_core::declare_errors! {
    type_base: "https://riverbase.io/~/rs/error/",
    RXD_001 {
        status: 422,
        title: "Unprocessable Content",
        code: "RXD-001",
        message: "Collection name is required for RxDB notification channel.",
    },
    RXD_002 {
        status: 422,
        title: "Unprocessable Content",
        code: "RXD-002",
        message: "Invalid RxDB collection name.",
    },
    RXD_003 {
        status: 422,
        title: "Unprocessable Content",
        code: "RXD-003",
        message: "RxDB collection already registered.",
    },
}
