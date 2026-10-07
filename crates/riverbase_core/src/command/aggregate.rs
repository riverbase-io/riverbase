use std::future::Future;
use std::sync::Arc;

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use uuid::Uuid;

use super::message::CommandMessage;
use super::meta::{CommandKind, CommandMeta};
use crate::base::{
    apply_tenant_stamp, authorize_tenant_transfer, carry_row_tenant, AggregateContext,
    AggregateRoot, CommandId, DomainFields, RiverbaseResult, ScopeMap, TenantTransfer,
};
use crate::datastore::{DataStore, Expr, OrderSpec, Projection, ResourceName};
use crate::logstore::{
    append_activity, log_uuid_from_command_id, new_log_id, parse_optional_uuid, scope_uuid,
    ActivityEmitParams, ActivityMsgType, DomainLogStore, EventLogRecord, LogRowMeta,
    MessageLogRecord,
};

/// Prefer client `If-Match` / `_if_match` claim over the persisted snapshot etag for CAS.
fn apply_if_match_etag(mut rootobj: Value, context: &AggregateContext) -> Value {
    let etag = context
        .claims
        .get("_if_match")
        .or_else(|| context.claims.get("_etag"))
        .cloned();
    if let Some(etag) = etag {
        if let Some(obj) = rootobj.as_object_mut() {
            obj.insert("_etag".to_string(), etag);
        }
    }
    rootobj
}

fn row_visible(context: &AggregateContext, row: &Value) -> bool {
    context.row_visible(row)
}

fn tenant_not_found(resource: &str, id: &str) -> crate::base::RiverbaseError {
    crate::errors::CMD_007.with_data(format!("{resource}:{id}"))
}

/// Command aggregate opened for one command dispatch, parameterized by its [`DataStore`].
#[async_trait]
pub trait Aggregate<S: DataStore>: Send + Sync + Sized + 'static {
    /// Open.
    async fn open(
        statemgr: Arc<S>,
        logstore: DomainLogStore,
        message: &CommandMessage,
        meta: &CommandMeta,
        context: &AggregateContext,
    ) -> RiverbaseResult<Self>;

    /// Core.
    fn core(&self) -> &AggregateCore<S>;
    /// Core mut.
    fn core_mut(&mut self) -> &mut AggregateCore<S>;

    /// Take messages.
    fn take_messages(&mut self) -> Vec<(String, Value)> {
        std::mem::take(&mut self.core_mut().messages)
    }

    /// Action event keys this aggregate may enqueue (`Domain.Event` / DOM-044).
    fn registered_event_keys() -> &'static [&'static str] {
        &[]
    }

    /// Persist queued action events and enforce the action/event session contract.
    async fn finish_command_session(&mut self) -> RiverbaseResult<()> {
        self.core_mut().finish_command_session().await
    }
}

/// Shared command-session fields and infrastructure helpers.
pub struct AggregateCore<S: DataStore> {
    /// Ctx.
    pub ctx: AggregateContext,
    /// Cmd id.
    pub cmd_id: CommandId,
    /// Resource.
    pub resource: String,
    /// Scope.
    pub scope: ScopeMap,
    /// Aggroot.
    pub aggroot: Option<AggregateRoot>,
    /// Rootobj.
    pub rootobj: Option<Value>,
    /// Statemgr.
    pub statemgr: Arc<S>,
    /// Logstore.
    pub logstore: DomainLogStore,
    /// Messages.
    pub messages: Vec<(String, Value)>,
    action_depth: u32,
    actions_begun: u32,
    pending_events: Vec<EventLogRecord>,
    registered_events: &'static [&'static str],
}

impl<S: DataStore> Clone for AggregateCore<S> {
    fn clone(&self) -> Self {
        Self {
            ctx: self.ctx.clone(),
            cmd_id: self.cmd_id.clone(),
            resource: self.resource.clone(),
            scope: self.scope.clone(),
            aggroot: self.aggroot.clone(),
            rootobj: self.rootobj.clone(),
            statemgr: self.statemgr.clone(),
            logstore: self.logstore.clone(),
            messages: self.messages.clone(),
            action_depth: self.action_depth,
            actions_begun: self.actions_begun,
            pending_events: self.pending_events.clone(),
            registered_events: self.registered_events,
        }
    }
}

impl<S: DataStore> AggregateCore<S> {
    /// Open.
    pub async fn open(
        statemgr: Arc<S>,
        logstore: DomainLogStore,
        message: &CommandMessage,
        meta: &CommandMeta,
        context: AggregateContext,
    ) -> RiverbaseResult<Self> {
        if !meta.resources.is_empty() && !meta.resources.iter().any(|r| r == &message.resource) {
            return Err(crate::errors::CMD_003.with_data(format!(
                "command {} does not allow resource {}",
                message.cmdkey, message.resource
            )));
        }

        let (aggroot, rootobj) = match meta.kind {
            CommandKind::Object | CommandKind::ObjectLink | CommandKind::ObjectHook => {
                let aggroot = message.aggroot.clone().ok_or_else(|| {
                    crate::errors::CMD_004.with_data(format!(
                        "object command {} requires aggroot",
                        message.cmdkey
                    ))
                })?;
                if aggroot.identifier.is_empty() {
                    return Err(crate::errors::CMD_005
                        .with_data("object commands require aggroot identifier"));
                }
                if aggroot.resource != message.resource {
                    return Err(crate::errors::CMD_006.with_data(format!(
                        "aggroot resource {} does not match command resource {}",
                        aggroot.resource, message.resource
                    )));
                }
                if !context.tenant_scope_exempt {
                    context.require_tenant()?;
                }
                let rootobj = statemgr
                    .state_fetch(
                        Some(&context.unit_of_work),
                        &aggroot.resource,
                        &aggroot.identifier,
                    )
                    .await?
                    .filter(|row| row_visible(&context, row))
                    .ok_or_else(|| {
                        crate::errors::CMD_007.with_data(format!(
                            "aggregate {}:{}",
                            aggroot.resource, aggroot.identifier
                        ))
                    })?;
                let rootobj = apply_if_match_etag(rootobj, &context);
                (Some(aggroot), Some(rootobj))
            }
            CommandKind::Collection => {
                if message.aggroot.is_some() {
                    return Err(crate::errors::CMD_008.with_data(format!(
                        "collection command {} must not include aggroot",
                        message.cmdkey
                    )));
                }
                (None, None)
            }
        };

        Ok(Self {
            cmd_id: message.cmd_id.clone(),
            resource: message.resource.clone(),
            scope: message.scope.clone(),
            aggroot,
            rootobj,
            statemgr,
            logstore,
            messages: Vec::new(),
            action_depth: 0,
            actions_begun: 0,
            pending_events: Vec::new(),
            registered_events: &[],
            ctx: context,
        })
    }

    /// Execution context identifier.
    pub fn context(&self) -> &AggregateContext {
        &self.ctx
    }

    /// Cmd id.
    pub fn cmd_id(&self) -> &CommandId {
        &self.cmd_id
    }

    /// Resource.
    pub fn resource(&self) -> &str {
        &self.resource
    }

    /// Scope.
    pub fn scope(&self) -> &ScopeMap {
        &self.scope
    }

    /// Aggroot.
    pub fn aggroot(&self) -> Option<&AggregateRoot> {
        self.aggroot.as_ref()
    }

    /// Rootobj.
    pub fn rootobj(&self) -> Option<&Value> {
        self.rootobj.as_ref()
    }

    /// Statemgr.
    pub fn statemgr(&self) -> &Arc<S> {
        &self.statemgr
    }

    /// Logstore.
    pub fn logstore(&self) -> &DomainLogStore {
        &self.logstore
    }

    /// Install the domain event-key allow-list used by enqueue (`DOM-044`).
    pub fn set_registered_events(&mut self, keys: &'static [&'static str]) {
        self.registered_events = keys;
    }

    /// Bind the aggregate root after a collection create so the action event has an identifier.
    pub fn bind_aggroot(&mut self, resource: impl Into<String>, identifier: impl ToString) {
        self.aggroot = Some(AggregateRoot::new(resource, identifier.to_string()));
    }

    /// Enter an aggregate action. Required around every state write.
    pub fn begin_domain_action(&mut self, resources: &[&str]) -> RiverbaseResult<()> {
        if !resources.is_empty() {
            let current = self
                .aggroot
                .as_ref()
                .map(|root| root.resource.as_str())
                .unwrap_or(self.resource.as_str());
            if !resources.iter().any(|resource| *resource == current) {
                return Err(crate::errors::CMD_028
                    .with_data(format!("action does not allow resource {current}")));
            }
        }
        self.action_depth = self.action_depth.saturating_add(1);
        self.actions_begun = self.actions_begun.saturating_add(1);
        Ok(())
    }

    /// Leave the current aggregate action.
    pub fn end_domain_action(&mut self) {
        self.action_depth = self.action_depth.saturating_sub(1);
    }

    fn ensure_in_action(&self) -> RiverbaseResult<()> {
        if self.action_depth == 0 {
            return Err(crate::errors::DOM_016.with_data(self.resource.clone()));
        }
        Ok(())
    }

    fn ensure_event_registered(&self, event_type: &str) -> RiverbaseResult<()> {
        if self.registered_events.is_empty() {
            return Ok(());
        }
        if self.registered_events.iter().any(|key| *key == event_type) {
            return Ok(());
        }
        Err(crate::errors::DOM_044.with_data(json!({ "event_key": event_type })))
    }

    fn context_args(&self, input: Value) -> Value {
        json!({
            "input": input,
            "context_id": self.ctx.context_id,
            "profile_id": self.ctx.actor.profile_id,
            "user_id": self.ctx.actor.user_id,
            "organization_id": self.ctx.claims.get("organization_id").and_then(Value::as_str),
            "_tenant": self.ctx.actor.tenant,
            "roles": self.ctx.roles,
            "correlation_id": self.ctx.correlation_id,
        })
    }

    fn build_event_record(&self, event_type: &str, args: Value, data: Value) -> EventLogRecord {
        EventLogRecord {
            meta: LogRowMeta {
                id: new_log_id(),
                created: self.ctx.timestamp,
                creator: self.ctx.actor.profile_id,
            },
            domain: Some(self.ctx.namespace.clone()),
            event: event_type.to_string(),
            identifier: self
                .aggroot
                .as_ref()
                .and_then(|root| parse_optional_uuid(&root.identifier)),
            resource: Some(
                self.aggroot
                    .as_ref()
                    .map(|root| root.resource.clone())
                    .unwrap_or_else(|| self.resource.clone()),
            ),
            src_cmd: log_uuid_from_command_id(&self.cmd_id),
            args,
            data,
            tenant: self.ctx.actor.tenant,
        }
    }

    /// Queue the action event (`event` = action key). Persist happens in [`Self::finish_command_session`].
    pub async fn enqueue_action_event(
        &mut self,
        event_type: &str,
        input: Value,
        result: Value,
    ) -> RiverbaseResult<()> {
        self.ensure_in_action()?;
        self.ensure_event_registered(event_type)?;
        let record = self.build_event_record(event_type, self.context_args(input), result);
        self.pending_events.push(record);
        Ok(())
    }

    /// Drain queued events without persisting them (tests / Python `consume_events`).
    pub fn consume_events(&mut self) -> Vec<EventLogRecord> {
        std::mem::take(&mut self.pending_events)
    }

    /// Enforce the action contract and append queued events to `event_log`.
    pub async fn finish_command_session(&mut self) -> RiverbaseResult<()> {
        if self.action_depth != 0 {
            return Err(
                crate::errors::DOM_018.with_data(format!("action_depth={}", self.action_depth))
            );
        }
        if self.actions_begun > 0 && self.pending_events.is_empty() {
            return Err(
                crate::errors::DOM_017.with_data(format!("actions_begun={}", self.actions_begun))
            );
        }
        let events = self.consume_events();
        for record in events {
            self.logstore
                .events
                .append(Some(&self.ctx.unit_of_work), record)
                .await?;
        }
        if !self.pending_events.is_empty() {
            return Err(
                crate::errors::DOM_046.with_data(format!("pending={}", self.pending_events.len()))
            );
        }
        Ok(())
    }

    /// Parse an API resource name against the state store's registry.
    fn parse_resource(&self, resource: &str) -> RiverbaseResult<ResourceName> {
        self.statemgr.registry().parse(resource)
    }

    /// Fetch a single row by `resource`/`id`, deserialized into `R` (`None` when absent).
    ///
    /// Read proxy (not OCC) into the [`DataStore`]; writes must go through the event/audit
    /// methods (`upsert`, `update`, `create`, `action`).
    pub async fn fetch<R>(&self, resource: &str, id: &str) -> RiverbaseResult<Option<R>>
    where
        R: DeserializeOwned + Send + Sync + 'static,
    {
        let name = self.parse_resource(resource)?;
        if self.ctx.tenant_filter_active() {
            self.ctx.require_tenant()?;
        }
        let Some(row) = self
            .statemgr
            .state_fetch(Some(&self.ctx.unit_of_work), resource, id)
            .await?
        else {
            return Ok(None);
        };
        if !row_visible(&self.ctx, &row) {
            return Ok(None);
        }
        self.statemgr.fetch::<R>(name, id).await
    }

    /// Find exactly one row matching `filter`; errors if zero or more than one row matches.
    ///
    /// Read proxy (not OCC) into the [`DataStore`]. Tenant access uses
    /// [`crate::datastore::dsl::DataQuery::policy_filter`] (see [`Self::find_all`]).
    pub async fn find_one<R>(
        &self,
        resource: &str,
        filter: Option<Expr>,
        order: Option<Vec<OrderSpec>>,
        projection: Option<Projection>,
    ) -> RiverbaseResult<R>
    where
        R: DeserializeOwned + Send + Sync + 'static,
    {
        let name = self.parse_resource(resource)?;
        let policy_filter = self.tenant_policy_filter()?;
        self.statemgr
            .find_one_with_policy::<R>(name, filter, policy_filter, order, projection)
            .await
    }

    /// Find all rows matching `filter` (with optional `order`/`projection`).
    ///
    /// Read proxy (not OCC) into the [`DataStore`]. Tenant access is applied as
    /// [`crate::datastore::dsl::DataQuery::policy_filter`] so `_tenant` is not
    /// validated against the entity `order:` allowlist (`DAT-079`).
    pub async fn find_all<R>(
        &self,
        resource: &str,
        filter: Option<Expr>,
        order: Option<Vec<OrderSpec>>,
        projection: Option<Projection>,
    ) -> RiverbaseResult<Vec<R>>
    where
        R: DeserializeOwned + Send + Sync + 'static,
    {
        let name = self.parse_resource(resource)?;
        let policy_filter = self.tenant_policy_filter()?;
        self.statemgr
            .find_all_with_policy::<R>(name, filter, policy_filter, order, projection)
            .await
    }

    fn tenant_policy_filter(&self) -> RiverbaseResult<Option<Expr>> {
        if !self.ctx.tenant_filter_active() {
            return Ok(None);
        }
        let access = self.ctx.effective_tenant_access();
        if access.is_empty() {
            return Err(crate::errors::DOM_049.with_data("empty tenant access"));
        }
        Ok(access.filter_expr())
    }

    /// Require a stamp source without writing it. Updates must not re-home the row.
    fn require_stamp_source(&self) -> RiverbaseResult<Uuid> {
        self.ctx.require_tenant()
    }

    /// Audit patch for a new row (`_created`, `_updated`, `_creator`, `_tenant`, `_etag`, …).
    pub fn audit_created_patch(&self, id: Uuid) -> Value {
        let fields = DomainFields::for_insert(&self.ctx, id);
        let mut patch = json!({});
        fields.merge_created_into(&mut patch);
        patch
    }

    /// Audit patch for an update (`_updated`, `_updater`, `_etag`).
    pub fn audit_updated_patch(&self) -> Value {
        let mut fields = DomainFields::from_context(
            &self.ctx,
            Uuid::nil(), // id unused for update-only patch
        );
        fields.touch_from_context(&self.ctx);
        let mut patch = json!({});
        fields.merge_updated_into(&mut patch);
        patch
    }

    /// Upsert with domain audit fields merged from [`AggregateContext`].
    pub async fn upsert(&mut self, resource: &str, id: Uuid, mut data: Value) -> RiverbaseResult<()> {
        self.ensure_in_action()?;
        let id_str = id.to_string();
        let existing = self
            .statemgr
            .state_fetch(Some(&self.ctx.unit_of_work), resource, &id_str)
            .await?;
        let stored = match existing {
            Some(row) if row_visible(&self.ctx, &row) => Some(row),
            Some(_) => {
                return Err(tenant_not_found(resource, &id_str));
            }
            None => None,
        };
        let patch = if stored.is_some() {
            self.audit_updated_patch()
        } else {
            self.audit_created_patch(id)
        };
        merge_json_object(&mut data, patch);
        ensure_row_id(&mut data, id);
        if let Some(current) = stored.as_ref() {
            let _stamp = self.require_stamp_source()?;
            carry_row_tenant(&mut data, current);
        } else {
            apply_tenant_stamp(&mut data, self.require_stamp_source()?);
        }
        self.statemgr
            .state_upsert(&self.ctx.unit_of_work, resource, &id_str, data.clone())
            .await?;
        self.sync_rootobj_occ(resource, &id_str, &data);
        Ok(())
    }

    /// Update an existing row; returns [`crate::base::NotFoundError`] if the row is missing.
    ///
    /// `data` is treated as a **shallow patch** merged onto the current row (same semantics as
    /// [`crate::datastore::merge_json`]). Partial updates must not wipe omitted columns.
    ///
    /// Uses the aggregate root snapshot `_etag` when updating that root so concurrent commands
    /// are rejected. After a successful write, the snapshot token is advanced so later updates
    /// in the same command (e.g. confirm then capture) can chain.
    pub async fn update(&mut self, resource: &str, id: Uuid, data: Value) -> RiverbaseResult<()> {
        self.ensure_in_action()?;
        let id_str = id.to_string();
        let current = self
            .statemgr
            .state_fetch(Some(&self.ctx.unit_of_work), resource, &id_str)
            .await?
            .filter(|row| row_visible(&self.ctx, row))
            .ok_or_else(|| crate::errors::CMD_017.with_data(format!("{resource}:{id_str}")))?;
        let aggregate_snapshot = self
            .aggroot
            .as_ref()
            .filter(|root| root.resource == resource && root.identifier == id_str)
            .and(self.rootobj.as_ref());
        let expected_etag = aggregate_snapshot
            .or(Some(&current))
            .and_then(|row| row.get("_etag"))
            .and_then(Value::as_str)
            .ok_or_else(|| crate::errors::RDT_302.with_data(format!("{resource}:{id_str}")))?
            .to_string();
        let mut merged = current.clone();
        merge_json_object(&mut merged, data);
        merge_json_object(&mut merged, self.audit_updated_patch());
        ensure_row_id(&mut merged, id);
        let _stamp = self.require_stamp_source()?;
        carry_row_tenant(&mut merged, &current);
        let name = self.parse_resource(resource)?;
        self.statemgr
            .compare_and_swap(
                &self.ctx.unit_of_work,
                name,
                &id_str,
                &expected_etag,
                merged.clone(),
            )
            .await?;
        self.sync_rootobj_occ(resource, &id_str, &merged);
        Ok(())
    }

    /// Move a row to `destination`. Only the current `_tenant` may call this.
    ///
    /// A destination equal to the stored tenant returns without a write. Any other
    /// actor, or a row with no parseable `_tenant`, fails with `DOM-050`. The
    /// destination is the argument, never a field of a command payload.
    pub async fn transfer_tenant(
        &mut self,
        resource: &str,
        id: Uuid,
        destination: Uuid,
    ) -> RiverbaseResult<()> {
        self.ensure_in_action()?;
        let actor = self.require_stamp_source()?;
        let id_str = id.to_string();
        let current = self
            .statemgr
            .state_fetch(Some(&self.ctx.unit_of_work), resource, &id_str)
            .await?
            .filter(|row| row_visible(&self.ctx, row))
            .ok_or_else(|| crate::errors::CMD_007.with_data(format!("{resource}:{id_str}")))?;
        let TenantTransfer::Move(destination) =
            authorize_tenant_transfer(actor, &current, destination)?
        else {
            return Ok(());
        };
        let aggregate_snapshot = self
            .aggroot
            .as_ref()
            .filter(|root| root.resource == resource && root.identifier == id_str)
            .and(self.rootobj.as_ref());
        let expected_etag = aggregate_snapshot
            .or(Some(&current))
            .and_then(|row| row.get("_etag"))
            .and_then(Value::as_str)
            .ok_or_else(|| crate::errors::RDT_302.with_data(format!("{resource}:{id_str}")))?
            .to_string();
        let mut merged = current.clone();
        merge_json_object(&mut merged, self.audit_updated_patch());
        ensure_row_id(&mut merged, id);
        if let Some(obj) = merged.as_object_mut() {
            obj.insert("_tenant".to_string(), json!(destination.to_string()));
        }
        let name = self.parse_resource(resource)?;
        self.statemgr
            .compare_and_swap(
                &self.ctx.unit_of_work,
                name,
                &id_str,
                &expected_etag,
                merged.clone(),
            )
            .await?;
        self.sync_rootobj_occ(resource, &id_str, &merged);
        Ok(())
    }

    /// Advance the in-memory aggregate root OCC token after a successful write.
    fn sync_rootobj_occ(&mut self, resource: &str, id: &str, written: &Value) {
        let is_root = self
            .aggroot
            .as_ref()
            .is_some_and(|root| root.resource == resource && root.identifier == id);
        if !is_root {
            return;
        }
        let Some(root) = self.rootobj.as_mut().and_then(Value::as_object_mut) else {
            return;
        };
        for key in ["_etag", "_updated", "_updater", "_tenant"] {
            if let Some(value) = written.get(key) {
                root.insert(key.to_string(), value.clone());
            }
        }
    }

    /// Create a new row with domain audit fields (insert-only; does not check for duplicates).
    pub async fn create(
        &mut self,
        resource: &str,
        id: Uuid,
        mut data: Value,
    ) -> RiverbaseResult<Value> {
        self.ensure_in_action()?;
        ensure_row_id(&mut data, id);
        merge_json_object(&mut data, self.audit_created_patch(id));
        apply_tenant_stamp(&mut data, self.require_stamp_source()?);
        self.statemgr
            .state_create(&self.ctx.unit_of_work, resource, data)
            .await
    }

    /// Soft-remove a row. Must run inside an aggregate action.
    pub async fn remove(&mut self, resource: &str, id: &str) -> RiverbaseResult<()> {
        self.ensure_in_action()?;
        let current = self
            .statemgr
            .state_fetch(Some(&self.ctx.unit_of_work), resource, id)
            .await?
            .filter(|row| row_visible(&self.ctx, row))
            .ok_or_else(|| tenant_not_found(resource, id))?;
        let _ = current;
        let parsed = self.parse_resource(resource)?;
        self.statemgr
            .remove(&self.ctx.unit_of_work, parsed, id)
            .await
    }

    /// Run `work` inside an action and enqueue `event_type` with `event_payload` as `data`.
    pub async fn action<F, Fut, R>(
        &mut self,
        event_type: &str,
        event_payload: Value,
        work: F,
    ) -> RiverbaseResult<R>
    where
        F: FnOnce(&mut Self) -> Fut,
        Fut: Future<Output = RiverbaseResult<R>>,
    {
        self.begin_domain_action(&[])?;
        let result = work(self).await;
        let out = match result {
            Ok(value) => match self
                .enqueue_action_event(event_type, json!({}), event_payload)
                .await
            {
                Ok(()) => Ok(value),
                Err(error) => Err(error),
            },
            Err(error) => Err(error),
        };
        self.end_domain_action();
        out
    }

    /// Queue an extra event inside an open action. The action wrapper owns the primary event.
    pub async fn record_event(&mut self, event_type: &str, payload: Value) -> RiverbaseResult<()> {
        self.ensure_in_action()?;
        self.ensure_event_registered(event_type)?;
        let record = self.build_event_record(event_type, self.context_args(json!({})), payload);
        self.pending_events.push(record);
        Ok(())
    }

    /// Emit message.
    pub async fn emit_message(
        &mut self,
        topic: impl Into<String>,
        payload: Value,
    ) -> RiverbaseResult<()> {
        let topic = topic.into();
        let record = MessageLogRecord {
            meta: LogRowMeta {
                id: new_log_id(),
                created: self.ctx.timestamp,
                creator: self.ctx.actor.profile_id,
            },
            domain: self.ctx.namespace.clone(),
            src_cmd: log_uuid_from_command_id(&self.cmd_id),
            message: topic.clone(),
            data: payload.clone(),
            tenant: self.ctx.actor.tenant,
        };
        self.logstore
            .messages
            .append(Some(&self.ctx.unit_of_work), record)
            .await?;
        self.messages.push((topic, payload));
        Ok(())
    }

    /// Emit activity using the type key as the stored message (legacy).
    pub async fn emit_activity(
        &mut self,
        activity_type: impl Into<String>,
        payload: Value,
    ) -> RiverbaseResult<()> {
        let activity_type = activity_type.into();
        self.append_activity_record(activity_type.clone(), activity_type, payload)
            .await
    }

    /// Emit a preformatted activity sentence; `activity_type` is stored as `msglabel`.
    pub async fn emit_activity_entry(
        &mut self,
        activity_type: impl Into<String>,
        message: impl Into<String>,
        payload: Value,
    ) -> RiverbaseResult<()> {
        self.append_activity_record(activity_type.into(), message.into(), payload)
            .await
    }

    async fn append_activity_record(
        &mut self,
        activity_type: String,
        message: String,
        payload: Value,
    ) -> RiverbaseResult<()> {
        let params = ActivityEmitParams::new(
            self.ctx.namespace.clone(),
            self.aggroot
                .as_ref()
                .map(|root| root.resource.clone())
                .unwrap_or_else(|| self.resource.clone()),
            message,
        )
        .msglabel(activity_type)
        .msgtype(ActivityMsgType::SystemCall)
        .payload(payload)
        .identifier(
            self.aggroot
                .as_ref()
                .and_then(|root| parse_optional_uuid(&root.identifier)),
        )
        .context(Some(self.ctx.context_id))
        .src_cmd(Some(log_uuid_from_command_id(&self.cmd_id)))
        .creator(self.ctx.actor.profile_id)
        .timestamp(self.ctx.timestamp);
        let params = ActivityEmitParams {
            domain_sid: scope_uuid(&self.scope, "domain_sid"),
            domain_iid: scope_uuid(&self.scope, "domain_iid"),
            tenant: self.ctx.actor.tenant,
            ..params
        };
        append_activity(&self.logstore, Some(&self.ctx.unit_of_work), params).await
    }
}

fn ensure_row_id(data: &mut Value, id: Uuid) {
    if let Some(obj) = data.as_object_mut() {
        obj.insert("id".to_string(), json!(id.to_string()));
    }
}

/// Collect `event = "..."` keys from `#[domain_action]` source (DOM-044 inventory).
pub fn domain_action_event_keys(source: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut rest = source;
    while let Some(idx) = rest.find("#[domain_action") {
        let after = &rest[idx..];
        let Some(end) = after.find(']') else {
            break;
        };
        let attr = &after[..end];
        if let Some(event_at) = attr.find("event") {
            let after_event = &attr[event_at..];
            if let Some(eq) = after_event.find('=') {
                let after_eq = after_event[eq + 1..].trim();
                if let Some(quoted) = after_eq.strip_prefix('"') {
                    if let Some(endq) = quoted.find('"') {
                        keys.push(quoted[..endq].to_string());
                    }
                }
            }
        }
        rest = &after[end + 1..];
    }
    keys.sort();
    keys.dedup();
    keys
}

/// Action keys present in `source` but missing from `registered`.
pub fn missing_action_event_keys(source: &str, registered: &[&str]) -> Vec<String> {
    domain_action_event_keys(source)
        .into_iter()
        .filter(|key| !registered.contains(&key.as_str()))
        .collect()
}

fn merge_json_object(base: &mut Value, patch: Value) {
    let Some(base_obj) = base.as_object_mut() else {
        return;
    };
    let Some(patch_obj) = patch.as_object() else {
        return;
    };
    for (key, value) in patch_obj {
        base_obj.insert(key.clone(), value.clone());
    }
}

/// Embed [`AggregateCore`] in a generic domain aggregate and forward infrastructure methods.
///
/// The aggregate is expected to be generic over `S: DataStore` and hold its core in a
/// field named `core`, e.g. `struct TodoAggregate<S> { core: AggregateCore<S> }`.
#[macro_export]
macro_rules! aggregate_core_deref {
    ($ty:ident) => {
        impl<S: $crate::datastore::DataStore> std::ops::Deref for $ty<S> {
            type Target = $crate::command::AggregateCore<S>;

            fn deref(&self) -> &Self::Target {
                &self.core
            }
        }

        impl<S: $crate::datastore::DataStore> std::ops::DerefMut for $ty<S> {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.core
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::{domain_action_event_keys, missing_action_event_keys};

    #[test]
    fn parses_domain_action_event_keys() {
        let source = r#"
            #[domain_action(event = "goal.created")]
            pub async fn create() {}
            #[domain_action(event = "goal.updated", resources = ["goal"])]
            pub async fn replace() {}
        "#;
        assert_eq!(
            domain_action_event_keys(source),
            vec!["goal.created".to_string(), "goal.updated".to_string()]
        );
    }

    #[test]
    fn reports_unregistered_action_keys() {
        let source = r#"#[domain_action(event = "goal.created")]"#;
        assert_eq!(
            missing_action_event_keys(source, &["goal.updated"]),
            vec!["goal.created".to_string()]
        );
        assert!(missing_action_event_keys(source, &["goal.created"]).is_empty());
    }
}

#[cfg(test)]
mod transfer_tests {
    use std::any::Any;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use serde::de::DeserializeOwned;
    use serde::Serialize;
    use serde_json::{json, Value};
    use uuid::Uuid;

    use super::AggregateCore;
    use crate::base::{AggregateContext, CommandId, EngineContext, ScopeMap, TenantAccess};
    use crate::datastore::{CommandUnitOfWork, DataQuery, DataResult, DataStore, ResourceRegistry};
    use crate::logstore::{
        ActivityLogRecord, ClaimOutcome, CommandLogRecord, ContextLogRecord, DomainLogStore,
        EventLogRecord, IdempotencyClaim, IdempotencyScope, IdempotencyStore, MessageLogRecord,
        NoOpLogStore, NoOpOutboxStore, NoOpResponseLogStore, QueryLogRecord,
    };

    struct MapStore {
        registry: ResourceRegistry,
        rows: Mutex<HashMap<String, Value>>,
    }

    impl MapStore {
        fn new(row_id: &str, row: Value) -> Self {
            let mut rows = HashMap::new();
            rows.insert(format!("item:{row_id}"), row);
            Self {
                registry: ResourceRegistry::new(["item"]),
                rows: Mutex::new(rows),
            }
        }

        fn row(&self, row_id: &str) -> Value {
            self.rows
                .lock()
                .expect("rows")
                .get(&format!("item:{row_id}"))
                .cloned()
                .expect("row")
        }
    }

    #[async_trait]
    impl DataStore for MapStore {
        fn registry(&self) -> &ResourceRegistry {
            &self.registry
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn supports_command_transactions(&self) -> bool {
            false
        }

        async fn query_list<R>(&self, _query: &DataQuery) -> DataResult<Vec<R>>
        where
            R: DeserializeOwned + Send + Sync + 'static,
        {
            Ok(Vec::new())
        }

        async fn fetch<R>(
            &self,
            resource: crate::datastore::ResourceName,
            id: &str,
        ) -> DataResult<Option<R>>
        where
            R: DeserializeOwned + Send + Sync + 'static,
        {
            let key = format!("{}:{id}", resource.as_str());
            let rows = self.rows.lock().expect("rows");
            let Some(value) = rows.get(&key) else {
                return Ok(None);
            };
            Ok(Some(
                serde_json::from_value(value.clone()).expect("row decodes"),
            ))
        }

        async fn upsert<R>(
            &self,
            _uow: &CommandUnitOfWork,
            resource: crate::datastore::ResourceName,
            id: &str,
            data: R,
        ) -> DataResult<()>
        where
            R: Serialize + Send + Sync + 'static,
        {
            let value = serde_json::to_value(&data).expect("row encodes");
            self.rows
                .lock()
                .expect("rows")
                .insert(format!("{}:{id}", resource.as_str()), value);
            Ok(())
        }

        async fn remove(
            &self,
            _uow: &CommandUnitOfWork,
            resource: crate::datastore::ResourceName,
            id: &str,
        ) -> DataResult<()> {
            self.rows
                .lock()
                .expect("rows")
                .remove(&format!("{}:{id}", resource.as_str()));
            Ok(())
        }

        async fn invalidate(
            &self,
            uow: &CommandUnitOfWork,
            resource: crate::datastore::ResourceName,
            id: &str,
        ) -> DataResult<()> {
            self.remove(uow, resource, id).await
        }
    }

    struct DiscardIdempotency;

    #[async_trait]
    impl IdempotencyStore for DiscardIdempotency {
        async fn claim(
            &self,
            _scope: &IdempotencyScope,
            _actor: Option<Uuid>,
            _request_hash: &str,
        ) -> crate::base::RiverbaseResult<ClaimOutcome> {
            Ok(ClaimOutcome::Fresh(IdempotencyClaim::new()))
        }

        async fn complete(
            &self,
            _uow: Option<&CommandUnitOfWork>,
            _scope: &IdempotencyScope,
            _claim: &IdempotencyClaim,
            _cmd_id: &CommandId,
            _response: Value,
        ) -> crate::base::RiverbaseResult<()> {
            Ok(())
        }

        async fn fail(
            &self,
            _uow: Option<&CommandUnitOfWork>,
            _scope: &IdempotencyScope,
            _claim: &IdempotencyClaim,
            _cmd_id: &CommandId,
            _error: Value,
        ) -> crate::base::RiverbaseResult<()> {
            Ok(())
        }
    }

    fn noop_logs() -> DomainLogStore {
        DomainLogStore::new(
            Arc::new(NoOpLogStore::<ContextLogRecord>::new()),
            Arc::new(NoOpLogStore::<CommandLogRecord>::new()),
            Arc::new(NoOpLogStore::<EventLogRecord>::new()),
            Arc::new(NoOpLogStore::<MessageLogRecord>::new()),
            Arc::new(NoOpLogStore::<ActivityLogRecord>::new()),
            Arc::new(NoOpLogStore::<QueryLogRecord>::new()),
            Arc::new(NoOpResponseLogStore),
            Arc::new(DiscardIdempotency),
            Arc::new(NoOpOutboxStore),
        )
    }

    fn core(store: Arc<MapStore>, actor: Uuid) -> AggregateCore<MapStore> {
        let mut engine = EngineContext::new("rfx.setting").with_tenant_id(actor);
        engine.tenant_access = TenantAccess::All;
        AggregateCore {
            ctx: AggregateContext::from_engine(&engine, CommandId::new(), ScopeMap::new()),
            cmd_id: CommandId::new(),
            resource: "item".into(),
            scope: ScopeMap::new(),
            aggroot: None,
            rootobj: None,
            statemgr: store,
            logstore: noop_logs(),
            messages: Vec::new(),
            action_depth: 1,
            actions_begun: 1,
            pending_events: Vec::new(),
            registered_events: &[],
        }
    }

    #[tokio::test]
    async fn transfer_tenant_moves_row_for_source_only() {
        let owner = Uuid::new_v4();
        let destination = Uuid::new_v4();
        let id = Uuid::new_v4();
        let etag = Uuid::new_v4();
        let store = Arc::new(MapStore::new(
            &id.to_string(),
            json!({
                "id": id.to_string(),
                "_tenant": owner.to_string(),
                "_etag": etag.to_string(),
                "name": "kept",
            }),
        ));

        let mut owned = core(store.clone(), owner);
        owned
            .transfer_tenant("item", id, destination)
            .await
            .expect("source transfer");
        let moved = store.row(&id.to_string());
        assert_eq!(moved["_tenant"], json!(destination.to_string()));
        assert_eq!(moved["name"], json!("kept"));
        assert_ne!(moved["_etag"], json!(etag.to_string()));

        let outsider = Uuid::new_v4();
        let mut foreign = core(store.clone(), outsider);
        let denied = foreign
            .transfer_tenant("item", id, owner)
            .await
            .expect_err("outsider");
        assert_eq!(denied.errcode.as_str(), "DOM-050");
        assert_eq!(
            store.row(&id.to_string())["_tenant"],
            json!(destination.to_string())
        );

        let blank_id = Uuid::new_v4();
        let blank = Arc::new(MapStore::new(
            &blank_id.to_string(),
            json!({
                "id": blank_id.to_string(),
                "_etag": etag.to_string(),
                "name": "legacy",
            }),
        ));
        let mut unparsed = core(blank, owner);
        let err = unparsed
            .transfer_tenant("item", blank_id, destination)
            .await
            .expect_err("no tenant");
        assert_eq!(err.errcode.as_str(), "DOM-050");
    }
}
