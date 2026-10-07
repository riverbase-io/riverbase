use serde_json::Value;

use super::payload::{validate_command_payload, validate_unknown_fields, CommandPayload};
use super::target::CommandTarget;
use crate::base::{AggregateRoot, CommandId, RiverbaseResult, ScopeMap};

/// Command envelope passed to handlers.
#[derive(Debug, Clone)]
pub struct CommandMessage {
    /// Cmd id.
    pub cmd_id: CommandId,
    /// Cmdkey.
    pub cmdkey: String,
    /// Command or event payload.
    pub payload: Value,
    /// Resource.
    pub resource: String,
    /// Scope.
    pub scope: ScopeMap,
    /// Aggroot.
    pub aggroot: Option<AggregateRoot>,
}

impl CommandMessage {
    /// Construct a new value.
    pub fn new(cmdkey: impl Into<String>, payload: Value, target: CommandTarget) -> Self {
        let cmdkey = cmdkey.into();
        match target {
            CommandTarget::Object(aggroot) => Self {
                cmd_id: CommandId::new(),
                cmdkey,
                payload,
                resource: aggroot.resource.clone(),
                scope: aggroot.scope.clone(),
                aggroot: Some(aggroot),
            },
            CommandTarget::Collection { resource, scope } => Self {
                cmd_id: CommandId::new(),
                cmdkey,
                payload,
                resource,
                scope,
                aggroot: None,
            },
        }
    }

    /// Set id and return self.
    pub fn with_id(mut self, cmd_id: CommandId) -> Self {
        self.cmd_id = cmd_id;
        self
    }

    /// Parse payload.
    pub fn parse_payload<P: CommandPayload>(&self, deny_unknown_fields: bool) -> RiverbaseResult<P> {
        if deny_unknown_fields {
            validate_unknown_fields::<P>(&self.payload)?;
        }
        let payload = serde_json::from_value(self.payload.clone())
            .map_err(|e| crate::errors::WEB_001.with_data(e.to_string()))?;
        validate_command_payload(&payload)?;
        Ok(payload)
    }
}

#[cfg(test)]
mod tests {
    use garde::Validate;
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Debug, Deserialize, Serialize, Validate, JsonSchema)]
    struct SamplePayload {
        #[garde(length(min = 1))]
        title: String,
    }

    #[test]
    fn parse_payload_rejects_invalid_shape() {
        let msg = CommandMessage::new(
            "create-todo",
            serde_json::json!({ "title": 42 }),
            CommandTarget::collection("todo"),
        );
        assert!(msg.parse_payload::<SamplePayload>(false).is_err());
    }

    #[test]
    fn parse_payload_rejects_failed_garde_rules() {
        let msg = CommandMessage::new(
            "create-todo",
            serde_json::json!({ "title": "" }),
            CommandTarget::collection("todo"),
        );
        let err = msg.parse_payload::<SamplePayload>(false).unwrap_err();
        assert!(err.to_string().contains("CMD-002"));
    }

    #[test]
    fn parse_payload_accepts_valid_payload() {
        let msg = CommandMessage::new(
            "create-todo",
            serde_json::json!({ "title": "Buy milk" }),
            CommandTarget::collection("todo"),
        );
        let payload = msg
            .parse_payload::<SamplePayload>(false)
            .expect("valid payload");
        assert_eq!(payload.title, "Buy milk");
    }

    #[test]
    fn parse_payload_unknown_fields_follow_runtime_setting() {
        let msg = CommandMessage::new(
            "create-todo",
            serde_json::json!({ "title": "Buy milk", "unknown": true }),
            CommandTarget::collection("todo"),
        );
        assert!(msg.parse_payload::<SamplePayload>(false).is_ok());
        let err = msg.parse_payload::<SamplePayload>(true).unwrap_err();
        assert!(err.to_string().contains("WEB-001"));
    }
}
