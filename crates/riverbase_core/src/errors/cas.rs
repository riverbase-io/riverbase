//! `CAS` error catalogue.

crate::declare_errors! {
    type_base: "https://riverbase.io/~/rs/error/",
    CAS_010 {
        status: 403,
        title: "Forbidden",
        code: "CAS-010",
        message: "Activity not permitted.",
    },
}
