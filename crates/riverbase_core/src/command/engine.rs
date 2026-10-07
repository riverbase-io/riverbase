use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::base::{
    uuid_from_text, AggregateContext, AggregateRoot, CommandId, Engine, EngineContext, EngineKind,
    RiverbaseResult, ScopeMap,
};
use crate::logstore::{
    append_command_context, idempotency_request_hash, log_uuid_from_command_id,
    parse_optional_uuid, scope_uuid, ClaimOutcome, CommandLogRecord, CommandLogStatus,
    DomainLogStore, IdempotencyScope, LogRowMeta, OutboxRecord, ResponseRecord,
};
use async_trait::async_trait;
use ractor::{Actor, ActorProcessingErr, ActorRef};
use serde_json::Value;
use tokio::sync::oneshot;

type StoreAggregateMarker<S, A> = PhantomData<fn() -> (Arc<S>, A)>;
use tracing::{error, info};

use super::aggregate::Aggregate;
use super::batch::{BatchExecuteResult, BatchItemOutcome, BatchItemStatus, PreparedCommand};
use super::message::CommandMessage;
use super::meta::CommandMeta;
use super::msgbus::MessageBus;
use super::payload::CommandPayload;
use super::policy::{CommandPolicy, DefaultCommandPolicy};
use super::registry::CommandRegistry;
use super::target::CommandTarget;
use crate::datastore::{CommandUnitOfWork, DataStore};

/// Infrastructure wired into a command engine actor for store `S` and aggregate `A`.
pub struct CommandEngineArgs<S: DataStore, A: Aggregate<S>> {
    registry: CommandRegistry<S, A>,
    /// Logstore.
    pub logstore: DomainLogStore,
    /// Msgbus.
    pub msgbus: Arc<dyn MessageBus>,
    /// Statemgr.
    pub statemgr: Arc<S>,
    policies: Vec<Arc<dyn CommandPolicy>>,
    /// Domain namespace for spawn-time logging (from [`EngineContext::namespace`]).
    pub engine_name: Option<String>,
}

impl<S: DataStore + 'static, A: Aggregate<S>> Clone for CommandEngineArgs<S, A> {
    fn clone(&self) -> Self {
        Self {
            registry: self.registry.clone(),
            logstore: self.logstore.clone(),
            msgbus: self.msgbus.clone(),
            statemgr: self.statemgr.clone(),
            policies: self.policies.clone(),
            engine_name: self.engine_name.clone(),
        }
    }
}

impl<S: DataStore + 'static, A: Aggregate<S>> CommandEngineArgs<S, A> {
    /// Construct a new value.
    pub fn new(logstore: DomainLogStore, msgbus: Arc<dyn MessageBus>, statemgr: Arc<S>) -> Self {
        Self {
            registry: CommandRegistry::new(),
            logstore,
            msgbus,
            statemgr,
            policies: Vec::new(),
            engine_name: None,
        }
    }

    /// Stamp the owning domain namespace onto spawn logs.
    pub fn apply_engine_context(&mut self, ctx: &EngineContext) {
        self.engine_name = Some(ctx.namespace().to_string());
    }

    /// Register.
    pub fn register(
        &mut self,
        cmdkey: impl Into<String>,
        handler: Arc<dyn super::registry::ErasedCommandHandler<S, A>>,
    ) {
        self.registry.register(cmdkey, handler);
    }

    /// Register typed.
    pub fn register_typed<H>(&mut self, handler: H)
    where
        H: super::typed::TypedCommandHandler<A> + 'static,
    {
        self.registry.register_typed(handler);
    }

    /// Merge commands.
    pub fn merge_commands(&mut self, other: CommandRegistry<S, A>) {
        self.registry.merge(other);
    }

    /// Add policy.
    pub fn add_policy(&mut self, policy: Arc<dyn CommandPolicy>) {
        self.policies.push(policy);
    }

    /// Set policy and return self.
    pub fn with_policy(mut self, policy: Arc<dyn CommandPolicy>) -> Self {
        self.add_policy(policy);
        self
    }
}

#[derive(Debug)]
/// Command Engine Message enumeration.
pub enum CommandEngineMessage {
    /// List cmd.
    ListCmd {
        /// Reply.
        reply: oneshot::Sender<RiverbaseResult<Vec<String>>>,
    },
    /// Execute.
    Execute {
        /// Ctx.
        ctx: Box<EngineContext>,
        /// Cmdkey.
        cmdkey: String,
        /// Command or event payload.
        payload: Value,
        /// Target.
        target: CommandTarget,
        /// Reply.
        reply: oneshot::Sender<RiverbaseResult<Value>>,
    },
    /// Execute several prepared commands in one host transaction (internal batch UoW).
    ExecuteBatch {
        /// Ctx.
        ctx: Box<EngineContext>,
        /// Prepared items in order.
        items: Vec<PreparedCommand>,
        /// Reply.
        reply: oneshot::Sender<RiverbaseResult<BatchExecuteResult>>,
    },
}

struct CommandEngineRuntime<S: DataStore, A: Aggregate<S>> {
    registry: Arc<CommandRegistry<S, A>>,
    logstore: DomainLogStore,
    msgbus: Arc<dyn MessageBus>,
    statemgr: Arc<S>,
    policies: Vec<Arc<dyn CommandPolicy>>,
}

impl<S: DataStore, A: Aggregate<S>> Clone for CommandEngineRuntime<S, A> {
    fn clone(&self) -> Self {
        Self {
            registry: self.registry.clone(),
            logstore: self.logstore.clone(),
            msgbus: self.msgbus.clone(),
            statemgr: self.statemgr.clone(),
            policies: self.policies.clone(),
        }
    }
}

struct CommandEngineState<S: DataStore, A: Aggregate<S>> {
    runtime: CommandEngineRuntime<S, A>,
}

struct CommandEngineActor<S: DataStore, A: Aggregate<S>> {
    _marker: StoreAggregateMarker<S, A>,
}

#[ractor::async_trait]
impl<S: DataStore + 'static, A: Aggregate<S>> Actor for CommandEngineActor<S, A> {
    type Msg = CommandEngineMessage;
    type State = CommandEngineState<S, A>;
    type Arguments = CommandEngineRuntime<S, A>;

    async fn pre_start(
        &self,
        _selfref: ActorRef<Self::Msg>,
        runtime: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(CommandEngineState { runtime })
    }

    async fn handle(
        &self,
        _selfref: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            CommandEngineMessage::ListCmd { reply } => {
                let keys = state.runtime.registry.cmdkeys();
                let _ = reply.send(Ok(keys));
            }
            CommandEngineMessage::Execute {
                ctx,
                cmdkey,
                payload,
                target,
                reply,
            } => {
                let result =
                    run_execute(&state.runtime, ctx.as_ref(), &cmdkey, payload, target).await;
                let _ = reply.send(result);
            }
            CommandEngineMessage::ExecuteBatch { ctx, items, reply } => {
                let result = run_execute_batch(&state.runtime, ctx.as_ref(), items).await;
                let _ = reply.send(result);
            }
        }
        Ok(())
    }
}

async fn authorize_command<S: DataStore + 'static, A: Aggregate<S>>(
    runtime: &CommandEngineRuntime<S, A>,
    ctx: &EngineContext,
    cmdkey: &str,
    payload: &Value,
    target: &CommandTarget,
) -> RiverbaseResult<()> {
    for policy in &runtime.policies {
        policy.authorize(ctx, cmdkey, payload, target).await?;
    }
    if let Some(meta) = runtime.registry.command_meta(cmdkey) {
        crate::command::meta::authorize_command_roles(ctx, &meta)?;
    }
    Ok(())
}

fn riverbase_error_to_value(err: &crate::base::RiverbaseError) -> Value {
    serde_json::to_value(err).unwrap_or_else(|_| {
        serde_json::json!({
            "errcode": err.errcode.as_str(),
            "errmesg": err.errmesg.clone(),
        })
    })
}

fn batch_item_not_run(index: usize, cmdkey: &str) -> BatchItemOutcome {
    BatchItemOutcome {
        index,
        cmdkey: cmdkey.to_string(),
        immediate_status: BatchItemStatus::NotRun,
        final_status: BatchItemStatus::NotRun,
        result: None,
        error: None,
    }
}

fn batch_item_failed(
    index: usize,
    cmdkey: &str,
    err: &crate::base::RiverbaseError,
) -> BatchItemOutcome {
    BatchItemOutcome {
        index,
        cmdkey: cmdkey.to_string(),
        immediate_status: BatchItemStatus::Failed,
        final_status: BatchItemStatus::Failed,
        result: None,
        error: Some(riverbase_error_to_value(err)),
    }
}

fn batch_item_ok(index: usize, cmdkey: &str, result: Value) -> BatchItemOutcome {
    BatchItemOutcome {
        index,
        cmdkey: cmdkey.to_string(),
        immediate_status: BatchItemStatus::Ok,
        final_status: BatchItemStatus::Ok,
        result: Some(result),
        error: None,
    }
}

/// The one place batch-wide conclusion is decided. Called exactly once per batch, after it's
/// known whether the transaction actually rolled back: `rolled_back = false` (committed, or no
/// transaction to roll back) leaves every `final_status` equal to `immediate_status`;
/// `rolled_back = true` forces every attempted (non-`not_run`) item's `final_status` to
/// `Failed`, since a rolled-back transaction persisted nothing.
fn conclude_batch_outcomes(outcomes: &mut [BatchItemOutcome], rolled_back: bool) {
    for item in outcomes.iter_mut() {
        item.final_status = if rolled_back && item.immediate_status != BatchItemStatus::NotRun {
            BatchItemStatus::Failed
        } else {
            item.immediate_status
        };
    }
}

/// Run prepared commands in one unit of work. Per-item idempotency is skipped.
async fn run_execute_batch<S: DataStore + 'static, A: Aggregate<S>>(
    runtime: &CommandEngineRuntime<S, A>,
    ctx: &EngineContext,
    items: Vec<PreparedCommand>,
) -> RiverbaseResult<BatchExecuteResult> {
    ctx.require_tenant()?;

    if items.is_empty() {
        return Ok(BatchExecuteResult::from_outcomes(Vec::new()));
    }

    for (index, item) in items.iter().enumerate() {
        if let Err(err) =
            authorize_command(runtime, ctx, &item.cmdkey, &item.payload, &item.target).await
        {
            let outcomes = items
                .iter()
                .enumerate()
                .map(|(i, it)| {
                    if i == index {
                        batch_item_failed(i, &it.cmdkey, &err)
                    } else {
                        batch_item_not_run(i, &it.cmdkey)
                    }
                })
                .collect();
            return Ok(BatchExecuteResult::from_outcomes(outcomes));
        }
    }

    let mut outcomes = Vec::with_capacity(items.len());
    // Message/context of every item that ran and committed within the transaction, kept
    // around so a later commit failure can still persist a durable failure row per item.
    let mut committed_items: Vec<(usize, CommandMessage, AggregateContext)> = Vec::new();

    let unit_of_work = runtime
        .statemgr
        .begin_command_transaction()
        .await
        .map_err(|err| crate::errors::CMD_024.with_data(err.to_string()))?;

    if !unit_of_work.is_transactional() && runtime.statemgr.supports_command_transactions() {
        return Err(crate::errors::CMD_027
            .with_data("misconfigured DataStore: supports_command_transactions is true"));
    }

    let mut all_outbox = Vec::new();
    let mut failed = false;

    for (index, item) in items.iter().enumerate() {
        if failed {
            outcomes.push(batch_item_not_run(index, &item.cmdkey));
            continue;
        }

        let cmd_id = CommandId::new();
        let message = CommandMessage::new(&item.cmdkey, item.payload.clone(), item.target.clone())
            .with_id(cmd_id.clone());
        let mut context = AggregateContext::from_engine(ctx, cmd_id.clone(), message.scope.clone());
        context.unit_of_work = unit_of_work.clone();
        if let Some(meta) = runtime.registry.command_meta(&item.cmdkey) {
            context.tenant_scope_exempt = meta.tenant_scope_exempt();
        }

        let attempt = run_command_attempt(
            runtime,
            ctx,
            &item.cmdkey,
            &item.payload,
            &message,
            &context,
            &unit_of_work,
            None,
            None,
        )
        .await;

        match attempt {
            Ok(committed) => {
                all_outbox.extend(committed.outbox);
                outcomes.push(batch_item_ok(index, &item.cmdkey, committed.response));
                committed_items.push((index, message, context));
            }
            Err(err) => {
                outcomes.push(batch_item_failed(index, &item.cmdkey, &err));
                failed = true;
                if unit_of_work.is_transactional() {
                    // Rolled back below; the durable failure row is written outside the
                    // transaction so it survives.
                    persist_failed_attempt(
                        runtime,
                        ctx,
                        &item.cmdkey,
                        &item.payload,
                        &message,
                        &context,
                        None,
                        None,
                        &err,
                    )
                    .await;
                } else {
                    // No rollback exists for a non-transactional store: run_command_attempt
                    // already committed this item's Created/Running rows, so the terminal
                    // status is an UPDATE, not another INSERT.
                    persist_non_transactional_failure(runtime, &cmd_id, None, None, &err).await;
                }
            }
        }
    }

    if failed {
        // A rollback only happens (and only invalidates earlier successes) for a
        // transactional store; a non-transactional store never rolls back, so earlier items
        // that already ran really did persist and keep their own status.
        let rolled_back = unit_of_work.is_transactional();
        if rolled_back {
            if let Err(rollback_err) = unit_of_work.rollback().await {
                error!(
                    error = %rollback_err,
                    "failed to roll back prepared command batch transaction"
                );
            }
        }
        conclude_batch_outcomes(&mut outcomes, rolled_back);
        return Ok(BatchExecuteResult::from_outcomes(outcomes));
    }

    if unit_of_work.is_transactional() {
        match unit_of_work.commit().await {
            Ok(()) => {
                deliver_enqueued(runtime, &all_outbox).await;
                conclude_batch_outcomes(&mut outcomes, false);
                Ok(BatchExecuteResult::from_outcomes(outcomes))
            }
            Err(commit_err) => {
                let _ = unit_of_work.rollback().await;
                conclude_batch_outcomes(&mut outcomes, true);
                let err = crate::errors::CMD_025.with_data(commit_err.to_string());
                for (index, message, context) in &committed_items {
                    let item = &items[*index];
                    persist_failed_attempt(
                        runtime,
                        ctx,
                        &item.cmdkey,
                        &item.payload,
                        message,
                        context,
                        None,
                        None,
                        &err,
                    )
                    .await;
                }
                Ok(BatchExecuteResult::from_outcomes(outcomes))
            }
        }
    } else {
        deliver_enqueued(runtime, &all_outbox).await;
        conclude_batch_outcomes(&mut outcomes, false);
        Ok(BatchExecuteResult::from_outcomes(outcomes))
    }
}

async fn run_execute<S: DataStore + 'static, A: Aggregate<S>>(
    runtime: &CommandEngineRuntime<S, A>,
    ctx: &EngineContext,
    cmdkey: &str,
    payload: Value,
    target: CommandTarget,
) -> RiverbaseResult<Value> {
    ctx.require_tenant()?;
    authorize_command(runtime, ctx, cmdkey, &payload, &target).await?;

    let idempotency_scope = ctx
        .idempotency_key
        .as_ref()
        .map(|key| IdempotencyScope::new(&ctx.namespace, cmdkey, key));

    let idempotency_claim = if let Some(scope) = &idempotency_scope {
        let request_hash = idempotency_request_hash(cmdkey, &payload, &target);
        match runtime
            .logstore
            .idempotency
            .claim(scope, ctx.actor.profile_id, &request_hash)
            .await?
        {
            ClaimOutcome::Completed(response) => return Ok(response),
            ClaimOutcome::InFlight => {
                return Err(crate::errors::CMD_021
                    .with_data(serde_json::json!({ "idempotency_key": scope.key })));
            }
            ClaimOutcome::Mismatch => {
                return Err(crate::errors::CMD_022
                    .with_data(serde_json::json!({ "idempotency_key": scope.key })));
            }
            ClaimOutcome::Failed(error) => {
                return Err(crate::errors::CMD_023.with_data(serde_json::json!({
                    "idempotency_key": scope.key,
                    "prior_error": error,
                })));
            }
            ClaimOutcome::Fresh(claim) => Some(claim),
        }
    } else {
        None
    };

    let cmd_id = CommandId::new();
    let message = CommandMessage::new(cmdkey, payload.clone(), target).with_id(cmd_id.clone());
    let unit_of_work = runtime
        .statemgr
        .begin_command_transaction()
        .await
        .map_err(|err| crate::errors::CMD_024.with_data(err.to_string()))?;

    if !unit_of_work.is_transactional() && runtime.statemgr.supports_command_transactions() {
        return Err(crate::errors::CMD_027
            .with_data("misconfigured DataStore: supports_command_transactions is true"));
    }

    let mut context = AggregateContext::from_engine(ctx, cmd_id.clone(), message.scope.clone());
    context.unit_of_work = unit_of_work.clone();
    if let Some(meta) = runtime.registry.command_meta(cmdkey) {
        context.tenant_scope_exempt = meta.tenant_scope_exempt();
    }

    let attempt = run_command_attempt(
        runtime,
        ctx,
        cmdkey,
        &payload,
        &message,
        &context,
        &unit_of_work,
        idempotency_scope.as_ref(),
        idempotency_claim.as_ref(),
    )
    .await;

    if unit_of_work.is_transactional() {
        match attempt {
            Ok(committed) => {
                if let Err(commit_err) = unit_of_work.commit().await {
                    let _ = unit_of_work.rollback().await;
                    let err = crate::errors::CMD_025.with_data(commit_err.to_string());
                    persist_failed_attempt(
                        runtime,
                        ctx,
                        cmdkey,
                        &payload,
                        &message,
                        &context,
                        idempotency_scope.as_ref(),
                        idempotency_claim.as_ref(),
                        &err,
                    )
                    .await;
                    return Err(err);
                }
                deliver_enqueued(runtime, &committed.outbox).await;
                Ok(committed.response)
            }
            Err(err) => {
                if let Err(rollback_err) = unit_of_work.rollback().await {
                    error!(
                        command_id = %cmd_id.0,
                        error = %rollback_err,
                        "failed to roll back command transaction"
                    );
                }
                persist_failed_attempt(
                    runtime,
                    ctx,
                    cmdkey,
                    &payload,
                    &message,
                    &context,
                    idempotency_scope.as_ref(),
                    idempotency_claim.as_ref(),
                    &err,
                )
                .await;
                Err(err)
            }
        }
    } else {
        match attempt {
            Ok(committed) => {
                deliver_enqueued(runtime, &committed.outbox).await;
                Ok(committed.response)
            }
            Err(err) => {
                persist_non_transactional_failure(
                    runtime,
                    &cmd_id,
                    idempotency_scope.as_ref(),
                    idempotency_claim.as_ref(),
                    &err,
                )
                .await;
                Err(err)
            }
        }
    }
}

struct CommittedCommand {
    response: Value,
    outbox: Vec<OutboxRecord>,
}

#[allow(clippy::too_many_arguments)]
async fn run_command_attempt<S: DataStore + 'static, A: Aggregate<S>>(
    runtime: &CommandEngineRuntime<S, A>,
    ctx: &EngineContext,
    cmdkey: &str,
    payload: &Value,
    message: &CommandMessage,
    context: &AggregateContext,
    unit_of_work: &CommandUnitOfWork,
    idempotency_scope: Option<&IdempotencyScope>,
    idempotency_claim: Option<&crate::logstore::IdempotencyClaim>,
) -> RiverbaseResult<CommittedCommand> {
    let uow = Some(unit_of_work);
    append_command_context(
        &runtime.logstore,
        context,
        ctx,
        cmdkey,
        &message.resource,
        payload,
        Some(unit_of_work),
    )
    .await?;

    let command_log_id = log_uuid_from_command_id(&message.cmd_id);
    runtime
        .logstore
        .commands
        .append(
            uow,
            command_log_record(
                ctx,
                cmdkey,
                payload,
                message,
                context,
                CommandLogStatus::Created,
            ),
        )
        .await?;
    runtime
        .logstore
        .commands
        .set_status(uow, command_log_id, CommandLogStatus::Running)
        .await?;

    let dispatch = runtime
        .registry
        .dispatch(
            runtime.statemgr.clone(),
            runtime.logstore.clone(),
            message.clone(),
            context,
        )
        .await?;
    let outbox = dispatch
        .bus_messages
        .iter()
        .enumerate()
        .map(|(index, (topic, bus_payload))| OutboxRecord {
            id: uuid_from_text(&format!("outbox:{}:{index}:{topic}", message.cmd_id.0)),
            created: context.timestamp,
            src_cmd: command_log_id,
            topic: topic.clone(),
            payload: bus_payload.clone(),
            attempts: 0,
        })
        .collect::<Vec<_>>();
    for record in &outbox {
        runtime.logstore.outbox.enqueue(uow, record.clone()).await?;
    }

    runtime
        .logstore
        .responses
        .append(
            uow,
            ResponseRecord {
                cmd_id: message.cmd_id.clone(),
                payload: dispatch.response.clone(),
            },
        )
        .await?;
    if let (Some(scope), Some(claim)) = (idempotency_scope, idempotency_claim) {
        runtime
            .logstore
            .idempotency
            .complete(
                uow,
                scope,
                claim,
                &message.cmd_id,
                dispatch.response.clone(),
            )
            .await?;
    }
    runtime
        .logstore
        .commands
        .set_status(uow, command_log_id, CommandLogStatus::Success)
        .await?;

    Ok(CommittedCommand {
        response: dispatch.response,
        outbox,
    })
}

fn command_log_record(
    _ctx: &EngineContext,
    cmdkey: &str,
    payload: &Value,
    message: &CommandMessage,
    context: &AggregateContext,
    status: CommandLogStatus,
) -> CommandLogRecord {
    CommandLogRecord {
        meta: LogRowMeta {
            id: log_uuid_from_command_id(&message.cmd_id),
            created: context.timestamp,
            creator: context.actor.profile_id,
        },
        domain: context.namespace.clone(),
        identifier: message
            .aggroot
            .as_ref()
            .and_then(|root| parse_optional_uuid(&root.identifier)),
        resource: message.resource.clone(),
        revision: 1,
        command: cmdkey.to_string(),
        domain_sid: scope_uuid(&message.scope, "domain_sid"),
        domain_iid: scope_uuid(&message.scope, "domain_iid"),
        payload: payload.clone(),
        context: context.context_id,
        status,
        tenant: context.actor.tenant,
    }
}

#[allow(clippy::too_many_arguments)]
async fn persist_failed_attempt<S: DataStore + 'static, A: Aggregate<S>>(
    runtime: &CommandEngineRuntime<S, A>,
    ctx: &EngineContext,
    cmdkey: &str,
    payload: &Value,
    message: &CommandMessage,
    context: &AggregateContext,
    idempotency_scope: Option<&IdempotencyScope>,
    idempotency_claim: Option<&crate::logstore::IdempotencyClaim>,
    err: &crate::base::RiverbaseError,
) {
    // The command transaction is finished or about to roll back. A write on that
    // handle fails with DAT-027, which the log store used to report as DAT-010.
    if let Err(log_err) = append_command_context(
        &runtime.logstore,
        context,
        ctx,
        cmdkey,
        &message.resource,
        payload,
        None,
    )
    .await
    {
        error!(command_id = %message.cmd_id.0, error = %log_err, "failed to persist failed command context");
    }
    if let Err(log_err) = runtime
        .logstore
        .commands
        .append(
            None,
            command_log_record(
                ctx,
                cmdkey,
                payload,
                message,
                context,
                CommandLogStatus::Errored,
            ),
        )
        .await
    {
        error!(command_id = %message.cmd_id.0, error = %log_err, "failed to persist failed command log");
    }
    persist_idempotency_failure(
        runtime,
        &message.cmd_id,
        idempotency_scope,
        idempotency_claim,
        err,
    )
    .await;
}

async fn persist_non_transactional_failure<S: DataStore + 'static, A: Aggregate<S>>(
    runtime: &CommandEngineRuntime<S, A>,
    cmd_id: &CommandId,
    idempotency_scope: Option<&IdempotencyScope>,
    idempotency_claim: Option<&crate::logstore::IdempotencyClaim>,
    err: &crate::base::RiverbaseError,
) {
    if let Err(status_err) = runtime
        .logstore
        .commands
        .set_status(
            None,
            log_uuid_from_command_id(cmd_id),
            CommandLogStatus::Errored,
        )
        .await
    {
        error!(command_id = %cmd_id.0, error = %status_err, "failed to persist terminal command status");
    }
    persist_idempotency_failure(runtime, cmd_id, idempotency_scope, idempotency_claim, err).await;
}

async fn persist_idempotency_failure<S: DataStore + 'static, A: Aggregate<S>>(
    runtime: &CommandEngineRuntime<S, A>,
    cmd_id: &CommandId,
    idempotency_scope: Option<&IdempotencyScope>,
    idempotency_claim: Option<&crate::logstore::IdempotencyClaim>,
    err: &crate::base::RiverbaseError,
) {
    if let (Some(scope), Some(claim)) = (idempotency_scope, idempotency_claim) {
        let error_payload = serde_json::to_value(err).unwrap_or_else(|_| {
            serde_json::json!({
                "errcode": err.errcode.as_str(),
                "errmesg": err.errmesg.clone(),
            })
        });
        if let Err(idempotency_err) = runtime
            .logstore
            .idempotency
            .fail(None, scope, claim, cmd_id, error_payload)
            .await
        {
            error!(command_id = %cmd_id.0, error = %idempotency_err, "failed to persist terminal idempotency status");
        }
    }
}

async fn deliver_enqueued<S: DataStore + 'static, A: Aggregate<S>>(
    runtime: &CommandEngineRuntime<S, A>,
    records: &[OutboxRecord],
) {
    for record in records {
        match runtime
            .msgbus
            .publish(&record.topic, record.payload.clone())
            .await
        {
            Ok(()) => {
                if let Err(mark_err) = runtime.logstore.outbox.mark_published(record.id).await {
                    error!(outbox_id = %record.id, error = %mark_err, "failed to mark delivered outbox record");
                }
            }
            Err(publish_err) => {
                error!(
                    outbox_id = %record.id,
                    topic = %record.topic,
                    error = %publish_err,
                    "outbox delivery deferred for retry"
                );
                if let Err(mark_err) = runtime
                    .logstore
                    .outbox
                    .mark_failed(
                        record.id,
                        &publish_err.to_string(),
                        Duration::from_secs(1),
                        10,
                    )
                    .await
                {
                    error!(outbox_id = %record.id, error = %mark_err, "failed to reschedule outbox record");
                }
            }
        }
    }
}

/// Command engine specialized for store `S` and domain aggregate `A`.
pub struct CommandEngine<S: DataStore, A: Aggregate<S>> {
    actors: Vec<ActorRef<CommandEngineMessage>>,
    next_actor: Arc<AtomicUsize>,
    registry: Arc<CommandRegistry<S, A>>,
    _marker: StoreAggregateMarker<S, A>,
}

impl<S: DataStore, A: Aggregate<S>> Clone for CommandEngine<S, A> {
    fn clone(&self) -> Self {
        Self {
            actors: self.actors.clone(),
            next_actor: self.next_actor.clone(),
            registry: self.registry.clone(),
            _marker: PhantomData,
        }
    }
}

impl<S: DataStore + 'static, A: Aggregate<S>> CommandEngine<S, A> {
    /// Spawn.
    pub async fn spawn(args: CommandEngineArgs<S, A>) -> RiverbaseResult<Self> {
        Self::spawn_with_size(1, args).await
    }

    /// Spawn with size.
    pub async fn spawn_with_size(
        size: usize,
        args: CommandEngineArgs<S, A>,
    ) -> RiverbaseResult<Self> {
        let size = size.max(1);
        let engine = args.engine_name.as_deref().unwrap_or("unknown");
        info!(engine, size, "command engine actor pool configured");
        let registry = Arc::new(args.registry);
        let policies = if args.policies.is_empty() {
            vec![Arc::new(DefaultCommandPolicy) as Arc<dyn CommandPolicy>]
        } else {
            args.policies
        };
        let runtime = CommandEngineRuntime {
            registry: registry.clone(),
            logstore: args.logstore,
            msgbus: args.msgbus,
            statemgr: args.statemgr,
            policies,
        };
        let mut actors = Vec::with_capacity(size);
        for _ in 0..size {
            let (actor, _) = Actor::spawn(
                None,
                CommandEngineActor::<S, A> {
                    _marker: PhantomData,
                },
                runtime.clone(),
            )
            .await
            .map_err(|e| crate::errors::CMD_010.with_data(e.to_string()))?;
            actors.push(actor);
        }
        Ok(Self {
            actors,
            next_actor: Arc::new(AtomicUsize::new(0)),
            registry,
            _marker: PhantomData,
        })
    }

    fn pick_actor(&self) -> &ActorRef<CommandEngineMessage> {
        let idx = self.next_actor.fetch_add(1, Ordering::Relaxed);
        &self.actors[idx % self.actors.len()]
    }

    /// Command meta.
    pub fn command_meta(&self, cmdkey: &str) -> Option<CommandMeta> {
        self.registry.command_meta(cmdkey)
    }

    /// Command info.
    pub fn command_info(&self, cmdkey: &str) -> Option<Value> {
        self.registry.command_info(cmdkey)
    }

    /// Execute payload.
    pub async fn execute_payload<P: CommandPayload>(
        &self,
        ctx: &EngineContext,
        cmdkey: &str,
        payload: P,
        target: CommandTarget,
    ) -> RiverbaseResult<Value> {
        let value = serde_json::to_value(&payload)
            .map_err(|e| crate::errors::CMD_011.with_data(e.to_string()))?;
        self.execute(ctx, cmdkey, value, target).await
    }

    /// Execute.
    pub async fn execute(
        &self,
        ctx: &EngineContext,
        cmdkey: &str,
        payload: Value,
        target: CommandTarget,
    ) -> RiverbaseResult<Value> {
        let (tx, rx) = oneshot::channel();
        self.pick_actor()
            .send_message(CommandEngineMessage::Execute {
                ctx: Box::new(ctx.clone()),
                cmdkey: cmdkey.to_string(),
                payload,
                target,
                reply: tx,
            })
            .map_err(|e| crate::errors::CMD_012.with_data(e.to_string()))?;
        rx.await
            .map_err(|e| crate::errors::CMD_013.with_data(e.to_string()))?
    }

    /// Execute several prepared commands in one host command transaction (internal; skips idempotency).
    pub async fn execute_prepared_batch(
        &self,
        ctx: &EngineContext,
        items: Vec<PreparedCommand>,
    ) -> RiverbaseResult<BatchExecuteResult> {
        let (tx, rx) = oneshot::channel();
        self.pick_actor()
            .send_message(CommandEngineMessage::ExecuteBatch {
                ctx: Box::new(ctx.clone()),
                items,
                reply: tx,
            })
            .map_err(|e| crate::errors::CMD_030.with_data(e.to_string()))?;
        rx.await
            .map_err(|e| crate::errors::CMD_031.with_data(e.to_string()))?
    }

    /// Run a typed handler using [`TypedCommandHandler::meta`] and optional [`TypedCommandHandler::target`].
    pub async fn invoke<H>(
        &self,
        ctx: &EngineContext,
        payload: H::Payload,
        target: Option<CommandTarget>,
    ) -> RiverbaseResult<Value>
    where
        H: super::typed::TypedCommandHandler<A> + Default + 'static,
    {
        let handler = H::default();
        let meta = handler.meta();
        let target = target.or_else(|| handler.target(&payload)).ok_or_else(|| {
            crate::errors::CMD_014.with_data(format!(
                "command {} requires an explicit dispatch target",
                meta.key
            ))
        })?;
        self.execute_payload(ctx, &meta.key, payload, target).await
    }

    /// Execute object.
    pub async fn execute_object(
        &self,
        ctx: &EngineContext,
        cmdkey: &str,
        payload: Value,
        aggroot: AggregateRoot,
    ) -> RiverbaseResult<Value> {
        self.execute(ctx, cmdkey, payload, CommandTarget::Object(aggroot))
            .await
    }

    /// Execute collection.
    pub async fn execute_collection(
        &self,
        ctx: &EngineContext,
        cmdkey: &str,
        payload: Value,
        resource: impl Into<String>,
        scope: ScopeMap,
    ) -> RiverbaseResult<Value> {
        self.execute(
            ctx,
            cmdkey,
            payload,
            CommandTarget::Collection {
                resource: resource.into(),
                scope,
            },
        )
        .await
    }

    /// Returns the first actor in the pool (for transport wiring in tests).
    pub fn actor_ref(&self) -> ActorRef<CommandEngineMessage> {
        self.actors[0].clone()
    }
}

#[async_trait]
impl<S: DataStore + 'static, A: Aggregate<S>> Engine for CommandEngine<S, A> {
    fn kind(&self) -> EngineKind {
        EngineKind::Command
    }

    async fn items(&self) -> RiverbaseResult<Vec<String>> {
        let (tx, rx) = oneshot::channel();
        self.pick_actor()
            .send_message(CommandEngineMessage::ListCmd { reply: tx })
            .map_err(|e| crate::errors::CMD_015.with_data(e.to_string()))?;
        rx.await
            .map_err(|e| crate::errors::CMD_016.with_data(e.to_string()))?
    }
}

#[cfg(test)]
mod batch_outcome_tests {
    use super::*;

    fn err() -> crate::base::RiverbaseError {
        crate::errors::CMD_018.with_data("boom")
    }

    #[test]
    fn commit_success_leaves_final_status_equal_to_immediate_status() {
        // 0: ok, 1: ok, 2: not_run should never appear here in practice (nothing failed), but
        // conclude_batch_outcomes must still be a no-op for whatever immediate_status it sees.
        let mut outcomes = vec![
            batch_item_ok(0, "a", serde_json::json!({"n": 1})),
            batch_item_ok(1, "b", serde_json::json!({"n": 2})),
        ];
        conclude_batch_outcomes(&mut outcomes, false);
        assert_eq!(outcomes[0].final_status, BatchItemStatus::Ok);
        assert_eq!(outcomes[1].final_status, BatchItemStatus::Ok);
        assert_eq!(outcomes[0].immediate_status, outcomes[0].final_status);
        assert!(BatchExecuteResult::from_outcomes(outcomes).ok);
    }

    #[test]
    fn mid_batch_rollback_forces_earlier_ok_items_to_final_failed() {
        // Item 0 ran and succeeded, item 1 failed, item 2 was never attempted.
        let mut outcomes = vec![
            batch_item_ok(0, "a", serde_json::json!({"n": 1})),
            batch_item_failed(1, "b", &err()),
            batch_item_not_run(2, "c"),
        ];
        conclude_batch_outcomes(&mut outcomes, true);

        assert_eq!(outcomes[0].immediate_status, BatchItemStatus::Ok);
        assert_eq!(
            outcomes[0].final_status,
            BatchItemStatus::Failed,
            "item 0 actually ran but the batch rolled back, so nothing it did persisted"
        );
        assert!(
            outcomes[0].result.is_some(),
            "immediate result is preserved for debugging even though final_status is failed"
        );

        assert_eq!(outcomes[1].immediate_status, BatchItemStatus::Failed);
        assert_eq!(outcomes[1].final_status, BatchItemStatus::Failed);

        assert_eq!(outcomes[2].immediate_status, BatchItemStatus::NotRun);
        assert_eq!(
            outcomes[2].final_status,
            BatchItemStatus::NotRun,
            "an item that never ran stays not_run regardless of rollback"
        );

        assert!(!BatchExecuteResult::from_outcomes(outcomes).ok);
    }

    #[test]
    fn commit_failure_after_full_success_forces_every_item_to_final_failed() {
        // All items succeeded in-transaction, but the commit itself failed and rolled back.
        let mut outcomes = vec![
            batch_item_ok(0, "a", serde_json::json!({"n": 1})),
            batch_item_ok(1, "b", serde_json::json!({"n": 2})),
        ];
        conclude_batch_outcomes(&mut outcomes, true);

        assert!(outcomes
            .iter()
            .all(|o| o.immediate_status == BatchItemStatus::Ok));
        assert!(outcomes
            .iter()
            .all(|o| o.final_status == BatchItemStatus::Failed));
        assert!(!BatchExecuteResult::from_outcomes(outcomes).ok);
    }

    #[test]
    fn non_transactional_failure_does_not_force_earlier_successes_to_failed() {
        // No rollback exists for a non-transactional store: an earlier item that already ran
        // really did persist, so its final_status must keep reflecting that.
        let mut outcomes = vec![
            batch_item_ok(0, "a", serde_json::json!({"n": 1})),
            batch_item_failed(1, "b", &err()),
            batch_item_not_run(2, "c"),
        ];
        conclude_batch_outcomes(&mut outcomes, false);

        assert_eq!(outcomes[0].final_status, BatchItemStatus::Ok);
        assert_eq!(outcomes[1].final_status, BatchItemStatus::Failed);
        assert_eq!(outcomes[2].final_status, BatchItemStatus::NotRun);
        assert!(!BatchExecuteResult::from_outcomes(outcomes).ok);
    }
}
