//! Error catalogue for `riverbase_task`.

riverbase_core::declare_errors! {
    type_base: "https://riverbase.io/~/rs/error/",
    TRK_001 {
        status: 500,
        title: "Internal Server Error",
        code: "TRK-001",
        message: "Failed to apply tracker database migrations.",
    },
    TRK_002 {
        status: 500,
        title: "Internal Server Error",
        code: "TRK-002",
        message: "Failed to connect to Postgres for tracker.",
    },
    TRK_003 {
        status: 500,
        title: "Internal Server Error",
        code: "TRK-003",
        message: "Failed to acquire a database connection for tracker migrations.",
    },
    TRK_005 {
        status: 500,
        title: "Internal Server Error",
        code: "TRK-005",
        message: "Failed to apply riverbase_task database migrations.",
    },
    TRK_017 {
        status: 500,
        title: "Internal Server Error",
        code: "TRK-017",
        message: "Failed to insert tracker worker row.",
    },
    TRK_019 {
        status: 500,
        title: "Internal Server Error",
        code: "TRK-019",
        message: "Failed to update tracker worker row.",
    },
    TRK_021 {
        status: 404,
        title: "Not Found",
        code: "TRK-021",
        message: "Tracker worker was not found.",
    },
    TRK_023 {
        status: 500,
        title: "Internal Server Error",
        code: "TRK-023",
        message: "Failed to insert worker job row.",
    },
    TRK_025 {
        status: 500,
        title: "Internal Server Error",
        code: "TRK-025",
        message: "Failed to update worker job row.",
    },
    TRK_027 {
        status: 404,
        title: "Not Found",
        code: "TRK-027",
        message: "Worker job was not found.",
    },
    TRK_029 {
        status: 500,
        title: "Internal Server Error",
        code: "TRK-029",
        message: "Failed to insert job relation row.",
    },
    TRK_031 {
        status: 500,
        title: "Internal Server Error",
        code: "TRK-031",
        message: "Failed to update job relation row.",
    },
    TRK_033 {
        status: 404,
        title: "Not Found",
        code: "TRK-033",
        message: "Job relation was not found.",
    },
    WRK_001 {
        status: 422,
        title: "Unprocessable Content",
        code: "WRK-001",
        message: "Stream bus is required for run_command_side_effect.",
    },
}
