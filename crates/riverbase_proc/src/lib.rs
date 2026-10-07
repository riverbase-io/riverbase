//! Proc-macros for riverbase_core command/query registration.

#[path = "proc_macro/mod.rs"]
mod macros;

/// Marks an aggregate **action**. On `Ok(result)` the framework enqueues one `event_log` row
/// whose `event` key is this action (not the command key). Persist happens when the command
/// session finishes.
///
/// The annotated async method must take `&mut self` (via `AggregateCore` deref). State writes
/// (`upsert` / `update` / `create` / `remove`) are only allowed inside the action. Activity
/// and messages stay explicit in the method body.
///
/// # Attributes
///
/// - `event` (required) — action event key, e.g. `"todo.created"`
/// - `resources` (optional) — allow-list of aggregate resources (`CMD-003` when mismatched)
///
/// # Example
///
/// ```ignore
/// #[domain_action(event = "todo.created", resources = ["todo"])]
/// pub async fn create(&mut self, title: &str) -> RiverbaseResult<Value> {
///     // upsert, emit_message, emit_activity — manual
///     Ok(json!({ "id": id, "title": title }))
/// }
/// ```
#[proc_macro_attribute]
pub fn domain_action(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    macros::domain_action(attr, item)
}

/// Build a namespace from a string literal, e.g. `riverbase_namespace!("riverbase.todo")`.
#[proc_macro]
pub fn riverbase_namespace(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    macros::riverbase_namespace(input)
}
