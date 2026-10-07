//! `RDT` error catalogue.

crate::declare_errors! {
    type_base: "https://riverbase.io/~/rs/error/",
    RDT_300 {
        status: 412,
        title: "Precondition Failed",
        code: "RDT-300",
        message: "Postgres If-Match precondition failed; the resource was modified.",
    },
    RDT_301 {
        status: 412,
        title: "Precondition Failed",
        code: "RDT-301",
        message: "Default datastore If-Match precondition failed; the resource was modified.",
    },
    RDT_302 {
        status: 412,
        title: "Precondition Failed",
        code: "RDT-302",
        message: "If-Match precondition failed; the resource was modified.",
    },
}
