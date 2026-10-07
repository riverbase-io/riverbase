//! `RTC` error catalogue.

riverbase_core::declare_errors! {
    type_base: "https://riverbase.io/~/rs/error/",
    RTC_001 {
        status: 500,
        title: "Internal Server Error",
        code: "RTC-001",
        message: "Failed to encode RTC JSON message.",
    },
    RTC_002 {
        status: 500,
        title: "Internal Server Error",
        code: "RTC-002",
        message: "Failed to encode RTC CBOR message.",
    },
    RTC_003 {
        status: 422,
        title: "Unprocessable Content",
        code: "RTC-003",
        message: "Invalid RTC JSON message.",
    },
    RTC_004 {
        status: 422,
        title: "Unprocessable Content",
        code: "RTC-004",
        message: "Invalid RTC CBOR message.",
    },
    RTC_005 {
        status: 422,
        title: "Unprocessable Content",
        code: "RTC-005",
        message: "Expected JSON text WebSocket frame.",
    },
    RTC_006 {
        status: 422,
        title: "Unprocessable Content",
        code: "RTC-006",
        message: "Expected CBOR binary WebSocket frame.",
    },
    RTC_007 {
        status: 422,
        title: "Unprocessable Content",
        code: "RTC-007",
        message: "Unexpected WebSocket control frame.",
    },
    RTC_008 {
        status: 422,
        title: "Unprocessable Content",
        code: "RTC-008",
        message: "WebSocket closed.",
    },
    RTC_010 {
        status: 400,
        title: "Bad Request",
        code: "RTC-010",
        message: "WebSocket bridge rejected the request.",
    },
    RTC_020 {
        status: 422,
        title: "Unprocessable Content",
        code: "RTC-020",
        message: "Invalid WebSocket message type.",
    },
    RTC_021 {
        status: 422,
        title: "Unprocessable Content",
        code: "RTC-021",
        message: "WebSocket message handler already registered.",
    },
    RTC_022 {
        status: 422,
        title: "Unprocessable Content",
        code: "RTC-022",
        message: "Invalid sendchan payload.",
    },
}
