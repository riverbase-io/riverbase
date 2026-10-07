use serde_json::Value;

/// Result of dispatching a command handler (response + bus publishes).
#[derive(Debug, Clone)]
pub struct CommandDispatchResult {
    /// Response.
    pub response: Value,
    /// Bus messages.
    pub bus_messages: Vec<(String, Value)>,
}
