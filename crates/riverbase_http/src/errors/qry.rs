//! `QRY` error catalogue.

riverbase_core::declare_errors! {
    type_base: "https://riverbase.io/~/rs/error/",
    QRY_124 {
        status: 403,
        title: "Forbidden",
        code: "QRY-124",
        message: "Row policy requires a bound claim.",
    },
}
