//! `AUT` error catalogue.

crate::declare_errors! {
    type_base: "https://riverbase.io/~/rs/error/",
    AUT_003 {
        status: 403,
        title: "Forbidden",
        code: "AUT-003",
        message: "The caller is not allowed to perform this action.",
    },
}
