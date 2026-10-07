use serde::{Deserialize, Serialize};
use ulid::Ulid;
use uuid::{uuid, Uuid};

/// Base namespace prefix for Rust services (`flrs.*`).
pub const BASE_NAMESPACE: &str = "flrs";

/// Namespace for deterministic UUID v5 names derived from text (`uuid_from_text`).
pub const NAME_UUID_NAMESPACE: Uuid = uuid!("48756e67-5472-616e-6750-687532303234");

/// Derive a stable UUID from `text` using [`NAME_UUID_NAMESPACE`] and UUID v5.
pub fn uuid_from_text(text: &str) -> Uuid {
    Uuid::new_v5(&NAME_UUID_NAMESPACE, text.as_bytes())
}

/// Convert a human-readable realm name to a deterministic UUID (not row `_tenant`).
pub fn realm_uuid(text: &str) -> Uuid {
    uuid_from_text(text)
}

/// Deterministic tenant UUID from a non-UUID config name (UUID v5 / SHA-1).
pub fn tenant_uuid(text: &str) -> Uuid {
    uuid_from_text(text)
}

/// Resolve `[riverbase] tenant` / `RIVERBASE_TENANT`: parse a UUID, or hash a name.
///
/// Empty input is `None`. A UUID literal is used as-is. Any other string is hashed
/// with [`tenant_uuid`] after a warning — prefer a UUID in config.
pub fn tenant_id_from_config(raw: &str) -> Option<Uuid> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(id) = Uuid::parse_str(raw) {
        return Some(id);
    }
    tracing::warn!(
        tenant = raw,
        "tenant is not a UUID; hashing the name to tenant_id (set a UUID to silence this)"
    );
    Some(tenant_uuid(raw))
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
/// Namespace structure.
pub struct Namespace(pub String);

impl Namespace {
    /// Construct a new value.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Fq.
    pub fn fq(&self, key: &str) -> crate::FqName {
        crate::fq(&self.0, key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
/// Entity id structure.
pub struct EntityId(pub String);

impl EntityId {
    /// Construct a new value.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Ulid.
    pub fn ulid() -> Self {
        Self(Ulid::new().to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
/// Command id structure.
pub struct CommandId(pub String);

impl Default for CommandId {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandId {
    /// Construct a new value.
    pub fn new() -> Self {
        Self(Ulid::new().to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
/// Tracker id structure.
pub struct TrackerId(pub Uuid);

impl Default for TrackerId {
    fn default() -> Self {
        Self::new()
    }
}

impl TrackerId {
    /// Construct a new value.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_from_text_is_deterministic() {
        let a = uuid_from_text("ui.date_format");
        let b = uuid_from_text("ui.date_format");
        assert_eq!(a, b);
        assert_ne!(a, uuid_from_text("ui.timezone"));
    }

    #[test]
    fn uuid_from_text_matches_uuid5_namespace() {
        let expected = Uuid::new_v5(&NAME_UUID_NAMESPACE, b"example");
        assert_eq!(uuid_from_text("example"), expected);
    }

    #[test]
    fn tenant_id_from_config_parses_uuid() {
        let id = Uuid::parse_str("00000000-0000-4000-8000-000000000012").unwrap();
        assert_eq!(tenant_id_from_config(&id.to_string()), Some(id));
    }

    #[test]
    fn tenant_id_from_config_hashes_name() {
        assert_eq!(tenant_id_from_config("abx"), Some(tenant_uuid("abx")));
        assert_eq!(tenant_id_from_config(""), None);
    }
}
