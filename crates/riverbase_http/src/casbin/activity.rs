use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A domain activity (matches `activity_type` written by `AggregateCore::emit_activity`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainActivity {
    /// Namespace.
    pub namespace: String,
    /// Activity type.
    pub activity_type: String,
    #[serde(default)]
    /// Resource.
    pub resource: String,
    #[serde(default)]
    /// Object id.
    pub object_id: String,
    #[serde(default)]
    /// Command or event payload.
    pub payload: Value,
}

impl DomainActivity {
    /// Construct a new value.
    pub fn new(namespace: impl Into<String>, activity_type: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            activity_type: activity_type.into(),
            resource: String::new(),
            object_id: String::new(),
            payload: Value::Null,
        }
    }

    /// Set resource and return self.
    pub fn with_resource(mut self, resource: impl Into<String>) -> Self {
        self.resource = resource.into();
        self
    }

    /// Set object id and return self.
    pub fn with_object_id(mut self, object_id: impl Into<String>) -> Self {
        self.object_id = object_id.into();
        self
    }

    /// Set payload and return self.
    pub fn with_payload(mut self, payload: Value) -> Self {
        self.payload = payload;
        self
    }
}

/// Authorization check input (subject + activity).
#[derive(Debug, Clone)]
pub struct ActivityRequest {
    /// Subject.
    pub subject: String,
    /// Activity.
    pub activity: DomainActivity,
}

impl ActivityRequest {
    /// Construct a new value.
    pub fn new(subject: impl Into<String>, activity: DomainActivity) -> Self {
        Self {
            subject: subject.into(),
            activity,
        }
    }
}

#[cfg(feature = "auth")]
/// Activity request ext trait.
pub trait ActivityRequestExt {
    /// Build from principal.
    fn from_principal(
        principal: &crate::auth::Principal,
        activity: DomainActivity,
    ) -> ActivityRequest;
}

#[cfg(feature = "auth")]
impl ActivityRequestExt for ActivityRequest {
    fn from_principal(
        principal: &crate::auth::Principal,
        activity: DomainActivity,
    ) -> ActivityRequest {
        ActivityRequest::new(principal.subject(), activity)
    }
}
