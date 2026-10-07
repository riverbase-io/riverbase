/// Runtime deployment mode for command/query services.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceMode {
    /// Command and query run in separate processes.
    Split,
    /// Command and query composed in one domain-oriented service.
    Coupled,
}
