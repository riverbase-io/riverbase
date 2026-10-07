//! `C00` error catalogue.

crate::declare_errors! {
    type_base: "https://riverbase.io/~/rs/error/",
    C00_006 {
        status: 422,
        title: "Unprocessable Content",
        code: "C00-006",
        message: "The request namespace does not match this service.",
    },
    C00_007 {
        status: 422,
        title: "Unprocessable Content",
        code: "C00-007",
        message: "The fully-qualified name is invalid.",
    },
}
