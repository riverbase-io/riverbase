//! `T00` error catalogue.

riverbase_core::declare_errors! {
    type_base: "https://riverbase.io/~/rs/error/",
    T00_102 {
        status: 403,
        title: "Forbidden",
        code: "T00-102",
        message: "Action is not allowed for this transport channel.",
    },
    T00_103 {
        status: 403,
        title: "Forbidden",
        code: "T00-103",
        message: "Transport channel rejected the action.",
    },
}
