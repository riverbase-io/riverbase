use uuid::Uuid;

use crate::base::CommandId;

/// Namespace for deterministic UUID mapping of non-UUID command ids (e.g. ULID).
const COMMAND_ID_NAMESPACE: Uuid = Uuid::from_bytes([
    0x6b, 0xa7, 0xb8, 0x10, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8,
]);

/// New audit row primary key (`_id`).
pub fn new_log_id() -> Uuid {
    Uuid::new_v4()
}

/// Map a command id string to a UUID for `src_cmd` / `command_log._id`.
pub fn log_uuid_from_command_id(cmd_id: &CommandId) -> Uuid {
    parse_optional_uuid(&cmd_id.0)
        .unwrap_or_else(|| Uuid::new_v5(&COMMAND_ID_NAMESPACE, cmd_id.0.as_bytes()))
}

/// Parse a string as UUID when possible.
pub fn parse_optional_uuid(value: &str) -> Option<Uuid> {
    Uuid::parse_str(value).ok()
}

/// Read a scope entry as UUID when the value is a valid UUID string.
pub fn scope_uuid(scope: &crate::base::ScopeMap, key: &str) -> Option<Uuid> {
    scope.get(key).and_then(|v| parse_optional_uuid(v))
}
