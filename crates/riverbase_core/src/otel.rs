//! Optional OpenTelemetry wiring stub ([OPS-05]).
//!
//! Tracing is always available via the `tracing` crate. Enable the `otel` feature and call
//! [`init_otel`] when an OTLP exporter should be attached. Without the feature this is a no-op.
//!
//! See [`docs/04-reference/13-opentelemetry-spike.md`](../../../../docs/04-reference/13-opentelemetry-spike.md).

/// Initialize OpenTelemetry export when the `otel` feature is enabled; otherwise no-op.
pub fn init_otel() {
    #[cfg(feature = "otel")]
    {
        tracing::info!(
            target: "riverbase_core::otel",
            "otel feature enabled; wire an OTLP exporter in the process (see docs/04-reference/13-opentelemetry-spike.md)"
        );
    }
    #[cfg(not(feature = "otel"))]
    {
        // Spike stub: production exporters are process-owned; see the reference doc.
    }
}
