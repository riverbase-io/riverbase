use std::any::TypeId;
use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap};
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, OnceLock, RwLock};

use garde::{Report, Validate};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::base::RiverbaseResult;

/// Per-command payload type (deserialize from HTTP/RPC JSON, serialize for typed execute).
pub trait CommandPayload:
    DeserializeOwned + Serialize + Validate<Context = ()> + JsonSchema + Send + Sync + 'static
{
}

impl<T> CommandPayload for T where
    T: DeserializeOwned + Serialize + Validate<Context = ()> + JsonSchema + Send + Sync + 'static
{
}

/// Opaque JSON object command payload (webhooks, passthrough commands).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(transparent)]
pub struct JsonObjectPayload(pub Map<String, serde_json::Value>);

impl Deref for JsonObjectPayload {
    type Target = Map<String, serde_json::Value>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for JsonObjectPayload {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl JsonSchema for JsonObjectPayload {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("JsonObjectPayload")
    }

    fn json_schema(_gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "object",
            "additionalProperties": true
        })
    }
}

impl Validate for JsonObjectPayload {
    type Context = ();

    fn validate_into(
        &self,
        _ctx: &Self::Context,
        _parent: &mut dyn FnMut() -> garde::Path,
        _report: &mut Report,
    ) {
    }
}

/// Run [`garde::Validate`] after a successful JSON deserialize.
pub fn validate_command_payload<P: Validate<Context = ()>>(payload: &P) -> RiverbaseResult<()> {
    payload
        .validate()
        .map_err(|e| crate::errors::CMD_002.with_data(e.to_string()))
}

static ALLOWED_PAYLOAD_KEYS: OnceLock<RwLock<HashMap<TypeId, Arc<BTreeSet<String>>>>> =
    OnceLock::new();

fn allowed_payload_keys<P: JsonSchema + 'static>() -> Arc<BTreeSet<String>> {
    let cache = ALLOWED_PAYLOAD_KEYS.get_or_init(|| RwLock::new(HashMap::new()));
    let type_id = TypeId::of::<P>();
    if let Some(keys) = cache
        .read()
        .expect("payload key cache poisoned")
        .get(&type_id)
    {
        return Arc::clone(keys);
    }
    let keys = Arc::new(extract_allowed_keys_from_schema::<P>());
    cache
        .write()
        .expect("payload key cache poisoned")
        .insert(type_id, Arc::clone(&keys));
    keys
}

fn extract_allowed_keys_from_schema<P: JsonSchema>() -> BTreeSet<String> {
    let schema = serde_json::to_value(schemars::schema_for!(P)).unwrap_or(Value::Null);
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .or_else(|| {
            schema
                .pointer("/schema/properties")
                .and_then(Value::as_object)
        });
    properties
        .map(|properties| properties.keys().cloned().collect())
        .unwrap_or_default()
}

/// Reject object keys absent from a typed payload's JSON Schema.
pub fn validate_unknown_fields<P: JsonSchema + 'static>(payload: &Value) -> RiverbaseResult<()> {
    let Some(input) = payload.as_object() else {
        return Ok(());
    };
    let allowed = allowed_payload_keys::<P>();
    let extras = input
        .keys()
        .filter(|key| !allowed.contains(key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if extras.is_empty() {
        return Ok(());
    }
    Err(crate::errors::WEB_001.with_data(format!("unknown fields: {}", extras.join(", "))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize, Serialize, Validate, JsonSchema)]
    struct CachedPayload {
        #[garde(length(min = 1))]
        title: String,
    }

    #[test]
    fn allowed_payload_keys_are_cached_per_type() {
        let first = allowed_payload_keys::<CachedPayload>();
        let second = allowed_payload_keys::<CachedPayload>();
        assert!(Arc::ptr_eq(&first, &second));
        assert!(first.contains("title"));
    }
}
