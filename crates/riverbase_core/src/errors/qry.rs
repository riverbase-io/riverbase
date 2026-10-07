//! `QRY` error catalogue.

crate::declare_errors! {
    type_base: "https://riverbase.io/~/rs/error/",
    QRY_001 {
        status: 404,
        title: "Not Found",
        code: "QRY-001",
        message: "Query resource is not registered.",
    },
    QRY_002 {
        status: 422,
        title: "Unprocessable Content",
        code: "QRY-002",
        message: "Item query requires an item id.",
    },
    QRY_006 {
        status: 500,
        title: "Internal Server Error",
        code: "QRY-006",
        message: "Query engine concurrency limit was closed.",
    },
    QRY_101 {
        status: 404,
        title: "Not Found",
        code: "QRY-101",
        message: "Query item was not found.",
    },
    QRY_110 {
        status: 422,
        title: "Unprocessable Content",
        code: "QRY-110",
        message: "Unknown composite operator.",
    },
    QRY_120 {
        status: 422,
        title: "Unprocessable Content",
        code: "QRY-120",
        message: "Text search is not allowed for this resource.",
    },
    QRY_121 {
        status: 403,
        title: "Forbidden",
        code: "QRY-121",
        message: "Query policy denied access to this resource.",
    },
    QRY_122 {
        status: 500,
        title: "Internal Server Error",
        code: "QRY-122",
        message: "Duplicate query resources were registered.",
    },
    QRY_123 {
        status: 500,
        title: "Internal Server Error",
        code: "QRY-123",
        message: "A policy-required query resource uses an executor that cannot enforce policy.",
    },
    QRY_125 {
        status: 500,
        title: "Internal Server Error",
        code: "QRY-125",
        message: "Sortable joined fields are not supported; mark the field no_sort.",
    },
    QRY_126 {
        status: 500,
        title: "Internal Server Error",
        code: "QRY-126",
        message: "Sortable field is not orderable on the storage entity.",
    },
    QRY_130 {
        status: 422,
        title: "Unprocessable Content",
        code: "QRY-130",
        message: "Query limit is out of range.",
    },
    QRY_141 {
        status: 403,
        title: "Forbidden",
        code: "QRY-141",
        message: "Query context is missing a tenant identifier.",
    },
    QRY_142 {
        status: 500,
        title: "Internal Server Error",
        code: "QRY-142",
        message: "A policy-required query resource must declare scope_policy or policy: public.",
    },
    QRY_143 {
        status: 422,
        title: "Unprocessable Content",
        code: "QRY-143",
        message: "Unknown query field.",
    },
    QRY_144 {
        status: 422,
        title: "Unprocessable Content",
        code: "QRY-144",
        message: "Unknown query operator.",
    },
    QRY_145 {
        status: 422,
        title: "Unprocessable Content",
        code: "QRY-145",
        message: "Operator is not allowed for this field.",
    },
    QRY_146 {
        status: 422,
        title: "Unprocessable Content",
        code: "QRY-146",
        message: "Unknown or non-sortable sort field.",
    },
}
