//! `CAS` error catalogue.

riverbase_core::declare_errors! {
    type_base: "https://riverbase.io/~/rs/error/",
    CAS_001 {
        status: 500,
        title: "Internal Server Error",
        code: "CAS-001",
        message: "Casbin authorization model configuration is invalid.",
    },
    CAS_002 {
        status: 500,
        title: "Internal Server Error",
        code: "CAS-002",
        message: "Casbin enforcer could not be initialized.",
    },
    CAS_003 {
        status: 500,
        title: "Internal Server Error",
        code: "CAS-003",
        message: "Failed to add Casbin policy.",
    },
    CAS_004 {
        status: 500,
        title: "Internal Server Error",
        code: "CAS-004",
        message: "Failed to add Casbin role mapping.",
    },
    CAS_005 {
        status: 500,
        title: "Internal Server Error",
        code: "CAS-005",
        message: "Casbin authorization enforcement failed.",
    },
    CAS_006 {
        status: 500,
        title: "Internal Server Error",
        code: "CAS-006",
        message: "Failed to load Casbin policies.",
    },
    CAS_007 {
        status: 500,
        title: "Internal Server Error",
        code: "CAS-007",
        message: "Failed to load Casbin role groupings.",
    },
    CAS_008 {
        status: 500,
        title: "Internal Server Error",
        code: "CAS-008",
        message: "Failed to ensure casbin_rule schema.",
    },
    CAS_009 {
        status: 500,
        title: "Internal Server Error",
        code: "CAS-009",
        message: "Casbin requires a Postgres domain runtime.",
    },
    CAS_011 {
        status: 403,
        title: "Forbidden",
        code: "CAS-011",
        message: "Activity not permitted.",
    },
    CAS_012 {
        status: 500,
        title: "Internal Server Error",
        code: "CAS-012",
        message: "Casbin policy pack drifted from the embedded copy.",
    },
    CAS_013 {
        status: 500,
        title: "Internal Server Error",
        code: "CAS-013",
        message: "Commands have no corresponding Casbin policy row.",
    },
    CAS_014 {
        status: 500,
        title: "Internal Server Error",
        code: "CAS-014",
        message: "Casbin is enabled but the policy pack maps zero activities while domains are mounted.",
    },
    CAS_015 {
        status: 403,
        title: "Forbidden",
        code: "CAS-015",
        message: "Activity not permitted: Unmapped Activity.",
    },
}
