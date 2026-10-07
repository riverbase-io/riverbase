//! `HOK` error catalogue.

riverbase_core::declare_errors! {
    type_base: "https://riverbase.io/~/rs/error/",
    HOK_001 {
        status: 422,
        title: "Unprocessable Content",
        code: "HOK-001",
        message: "Failed to serialize hook token.",
    },
    HOK_002 {
        status: 403,
        title: "Forbidden",
        code: "HOK-002",
        message: "Invalid hook token.",
    },
    HOK_003 {
        status: 403,
        title: "Forbidden",
        code: "HOK-003",
        message: "Hook token expired.",
    },
    HOK_004 {
        status: 403,
        title: "Forbidden",
        code: "HOK-004",
        message: "Invalid hook token.",
    },
    HOK_005 {
        status: 403,
        title: "Forbidden",
        code: "HOK-005",
        message: "Invalid hook token.",
    },
    HOK_006 {
        status: 403,
        title: "Forbidden",
        code: "HOK-006",
        message: "Invalid hook token.",
    },
    HOK_008 {
        status: 403,
        title: "Forbidden",
        code: "HOK-008",
        message: "Hook token has already been used.",
    },
    HOK_009 {
        status: 403,
        title: "Forbidden",
        code: "HOK-009",
        message: "SHA-1 hook tokens are no longer accepted.",
    },
    HOK_010 {
        status: 403,
        title: "Forbidden",
        code: "HOK-010",
        message: "Hook token payload does not match the signed parameters.",
    },
}
