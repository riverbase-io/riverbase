//! `LNK` error catalogue.

riverbase_core::declare_errors! {
    type_base: "https://riverbase.io/~/rs/error/",
    LNK_001 {
        status: 422,
        title: "Unprocessable Content",
        code: "LNK-001",
        message: "Failed to serialize command link token.",
    },
    LNK_002 {
        status: 403,
        title: "Forbidden",
        code: "LNK-002",
        message: "Invalid link token.",
    },
    LNK_003 {
        status: 403,
        title: "Forbidden",
        code: "LNK-003",
        message: "Link token expired.",
    },
    LNK_004 {
        status: 403,
        title: "Forbidden",
        code: "LNK-004",
        message: "Invalid link token.",
    },
    LNK_005 {
        status: 403,
        title: "Forbidden",
        code: "LNK-005",
        message: "Invalid link token.",
    },
    LNK_006 {
        status: 403,
        title: "Forbidden",
        code: "LNK-006",
        message: "Invalid link token.",
    },
    LNK_007 {
        status: 403,
        title: "Forbidden",
        code: "LNK-007",
        message: "Invalid link token.",
    },
    LNK_008 {
        status: 403,
        title: "Forbidden",
        code: "LNK-008",
        message: "Invalid link token.",
    },
    LNK_009 {
        status: 422,
        title: "Unprocessable Content",
        code: "LNK-009",
        message: "Failed to compress link token payload.",
    },
    LNK_010 {
        status: 422,
        title: "Unprocessable Content",
        code: "LNK-010",
        message: "Failed to finish compressing link token.",
    },
    LNK_011 {
        status: 403,
        title: "Forbidden",
        code: "LNK-011",
        message: "SHA-1 link tokens are no longer accepted.",
    },
}
