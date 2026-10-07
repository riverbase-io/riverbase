//! `WEB` error catalogue.

riverbase_core::declare_errors! {
    type_base: "https://riverbase.io/~/rs/error/",
    WEB_102 {
        status: 422,
        title: "Unprocessable Content",
        code: "WEB-102",
        message: "Query parameter must be a JSON object.",
    },
    WEB_103 {
        status: 413,
        title: "Payload Too Large",
        code: "WEB-103",
        message: "Request body exceeds the configured size limit.",
    },
    WEB_104 {
        status: 429,
        title: "Too Many Requests",
        code: "WEB-104",
        message: "Request rate limit exceeded.",
    },
}
