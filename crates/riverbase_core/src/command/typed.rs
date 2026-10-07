use std::marker::PhantomData;
use std::sync::Arc;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde_json::{json, Value};

use super::aggregate::Aggregate;
use super::message::CommandMessage;
use super::meta::CommandMeta;
use super::outcome::CommandDispatchResult;
use super::payload::CommandPayload;
use super::registry::ErasedCommandHandler;
use super::target::CommandTarget;
use crate::base::{AggregateContext, RiverbaseResult};
use crate::datastore::DataStore;
use crate::logstore::DomainLogStore;

type StoreAggregateMarker<S, A> = PhantomData<fn() -> (Arc<S>, A)>;

/// Command handler with typed payload; runs against the domain [`Aggregate`].
///
/// `A` is the concrete aggregate (e.g. `TodoAggregate<S>`); the store type `S` is
/// recovered through the [`Aggregate`] bound at erase time.
#[async_trait]
pub trait TypedCommandHandler<A>: Send + Sync {
    /// Typed command payload.
    type Payload: CommandPayload;

    /// Shared metadata.
    fn meta(&self) -> CommandMeta;

    /// JSON Schema for [`Self::Payload`] (`Command.Data.model_json_schema()` in Python).
    fn payload_schema() -> Value {
        payload_json_schema::<Self::Payload>()
    }

    /// JSON Schema for the command success `data` payload (OpenAPI / `.meta`).
    fn response_schema() -> Value {
        generic_object_schema()
    }

    /// Default dispatch target when the caller does not supply one (e.g. collection commands).
    fn target(&self, _payload: &Self::Payload) -> Option<CommandTarget> {
        None
    }

    /// Handle.
    async fn handle(&self, payload: Self::Payload, aggregate: &mut A) -> RiverbaseResult<Value>;
}

/// Erase handler.
pub fn erase_handler<S, A, H>(handler: H) -> Arc<dyn ErasedCommandHandler<S, A>>
where
    S: DataStore + 'static,
    A: Aggregate<S>,
    H: TypedCommandHandler<A> + 'static,
{
    Arc::new(ErasedTypedHandler {
        inner: handler,
        _marker: PhantomData,
    })
}

struct ErasedTypedHandler<S, A, H>
where
    S: DataStore,
    A: Aggregate<S>,
    H: TypedCommandHandler<A>,
{
    inner: H,
    _marker: StoreAggregateMarker<S, A>,
}

#[async_trait]
impl<S, A, H> ErasedCommandHandler<S, A> for ErasedTypedHandler<S, A, H>
where
    S: DataStore,
    A: Aggregate<S>,
    H: TypedCommandHandler<A> + 'static,
{
    fn command_meta(&self) -> CommandMeta {
        self.inner.meta()
    }

    fn command_info(&self) -> Value {
        let meta = self.inner.meta();
        meta.command_info_document(H::payload_schema(), H::response_schema())
    }

    async fn handle(
        &self,
        statemgr: Arc<S>,
        logstore: DomainLogStore,
        message: CommandMessage,
        context: &AggregateContext,
    ) -> RiverbaseResult<CommandDispatchResult> {
        let meta = self.inner.meta();
        let payload = message.parse_payload::<H::Payload>(context.deny_unknown_fields)?;
        let mut aggregate = A::open(statemgr, logstore, &message, &meta, context).await?;
        aggregate
            .core_mut()
            .set_registered_events(A::registered_event_keys());
        let response = self.inner.handle(payload, &mut aggregate).await?;
        aggregate.finish_command_session().await?;
        let bus_messages = aggregate.take_messages();
        Ok(CommandDispatchResult {
            response,
            bus_messages,
        })
    }
}

/// JSON Schema for a command payload (`Command.Data.model_json_schema()` in Python).
pub fn payload_json_schema<P: JsonSchema>() -> Value {
    normalize_payload_schema(
        serde_json::to_value(schemars::schema_for!(P)).unwrap_or_else(|_| generic_object_schema()),
    )
}

/// Normalize schemars output into a plain JSON Schema object for APIs and OpenAPI.
pub fn normalize_payload_schema(value: Value) -> Value {
    if value
        .as_object()
        .is_some_and(|o| o.contains_key("properties"))
    {
        return value;
    }
    if let Some(schema) = value.get("schema").filter(|s| s.is_object()) {
        return schema.clone();
    }
    if value.as_bool() == Some(true) {
        return generic_object_schema();
    }
    value
}

/// Generic object schema.
pub fn generic_object_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": true
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use garde::Validate;

    #[derive(Debug, serde::Deserialize, serde::Serialize, Validate, JsonSchema)]
    #[garde(allow_unvalidated)]
    struct SamplePayload {
        title: String,
        #[serde(default)]
        done: Option<bool>,
    }

    #[test]
    fn payload_json_schema_includes_properties() {
        let schema = payload_json_schema::<SamplePayload>();
        assert!(
            schema.pointer("/properties/title").is_some(),
            "expected title property, got {schema}"
        );
        assert_ne!(
            schema,
            generic_object_schema(),
            "schema serialization should not fall back to generic object"
        );
    }

    #[test]
    fn payload_json_schema_supports_uuid_fields() {
        use uuid::Uuid;

        #[derive(JsonSchema)]
        #[allow(dead_code)]
        struct UuidPayload {
            organization_id: Uuid,
            code: String,
        }

        let schema = payload_json_schema::<UuidPayload>();
        assert!(
            schema.pointer("/properties/organization_id").is_some(),
            "expected organization_id property, got {schema}"
        );
        assert_ne!(schema, generic_object_schema());
    }
}
