//! `IDM` error catalogue.

riverbase_core::declare_errors! {
    type_base: "https://riverbase.io/~/rs/error/",
    IDM_010 {
        status: 400,
        title: "Bad Request",
        code: "IDM-010",
        message: "Idempotency-Key exceeds maximum length of 128 characters.",
    },
}
