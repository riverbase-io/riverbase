//! Command-engine declarative macros.
//!
//! - [`command_engine!`](crate::command_engine) — handlers, optional engine + methods, or commands-only
//!
//! See `docs/02-design/command/domain-commands-macro.md`.

/// Declarative command handlers and optional domain command engine.
///
/// **Commands-only** (handlers + hidden register helper):
///
/// ```ignore
/// command_engine! {
///     commands PaymentAggregate {
///         command RegisterProvider { meta { ... } payload { ... } handle (...) }
///     }
/// }
/// ```
///
/// **Full** (todo-app style — engine struct, spawn, methods):
///
/// Notes on the DSL:
/// - `component` field types accept a full type expression (`Arc<dyn MessageBus>`).
///   Payload field types still need a single path or a `type` alias.
/// - Method parameter types are wrapped in `[ ... ]` and separated by `;`.
/// - Each method declares its `payload <expr>` and `target none | some(<expr>)`.
/// - Optional `domain_trait: manual` skips the generated [`DomainCommandEngine`]
///   impl (for engines that wrap `execute` with side effects). Default is generate.
/// - Optional `store: SomeStore` builds a non-generic engine for aggregates that
///   are not parameterized by `S` (for example `SettingAggregate` + `PgDataStore`).
/// - Generated engines include `spawn_empty` for query-only portals.
///
/// ```ignore
/// command_engine! {
///     engine TodoCommandEngine {
///         domain_trait: manual, // keep a hand-written DomainCommandEngine
///         aggregate: TodoAggregate,
///         component: { msgbus: TodoMessageBus },
///     }
///     commands { command CreateTodo { ... } }
///     methods (
///         create(title: [impl Into<String>]) for CreateTodo {
///             payload CreateTodoPayload { title: title.into() },
///             target none
///         }
///         set_done(id: [impl Into<String>]; done: [bool]) for UpdateTodo {
///             payload UpdateTodoPayload { done: Some(done), notes: None },
///             target some(CommandTarget::Object(...))
///         }
///     )
/// }
/// ```
///
/// Hand-written engines that only forward to `self.engine` can use
/// [`impl_domain_command_engine!`](crate::impl_domain_command_engine) instead of
/// copy-pasting the trait impl.
#[macro_export]
macro_rules! command_engine {
    // --- commands-only (handlers + hidden register) ---
    (
        commands $domain:ident {
            $($commands:tt)*
        }
    ) => {
        $crate::command_engine!(@commands_body $domain, { $($commands)* });
        $crate::command_engine!(@register_fn_hidden $domain, { $($commands)* });
    };

    // --- full, fixed store (non-generic aggregate, e.g. SettingAggregate + PgDataStore) ---
    (
        engine $engine:ident {
            $(domain_trait: $domain_trait:ident,)?
            $(
                meta {
                    $($pool_key:ident : $pool_size:literal),+ $(,)?
                }
            )?
            store: $store:ty,
            aggregate: $domain:ident,
            component: {
                $($component:ident : $component_ty:ty),* $(,)?
            } $(,)?
        }
        commands {
            $($commands:tt)*
        }
        methods (
            $($methods:tt)*
        )
    ) => {
        $crate::command_engine!(@commands_body_fixed $domain, { $($commands)* });

        $crate::paste::paste! {
            pub struct $engine {
                ctx: $crate::base::EngineContext,
                engine: $crate::command::CommandEngine<$store, $domain>,
                $(
                    #[allow(dead_code)]
                    $component: $component_ty,
                )*
            }

            impl Clone for $engine {
                fn clone(&self) -> Self {
                    Self {
                        ctx: self.ctx.clone(),
                        engine: self.engine.clone(),
                        $($component: self.$component.clone(),)*
                    }
                }
            }

            impl $engine {
                pub const ACTOR_POOL_SIZE: u32 = $crate::command_engine!(@pool_size $($($pool_key = $pool_size),+)?);
                pub const COMMAND_POOL_SIZE: u32 = Self::ACTOR_POOL_SIZE;

                pub async fn spawn(
                    ctx: $crate::base::EngineContext,
                    logstore: $crate::logstore::DomainLogStore,
                    $($component: $component_ty,)*
                    statemgr: ::std::sync::Arc<$store>,
                ) -> $crate::base::RiverbaseResult<Self> {
                    let mut args = $crate::command::CommandEngineArgs::new(
                        logstore,
                        $($component.clone(),)*
                        statemgr,
                    );
                    $crate::command_engine!(@register_handlers $domain, { $($commands)* }, args);
                    args.apply_engine_context(&ctx);
                    let engine = $crate::command::CommandEngine::spawn_with_size(
                        Self::ACTOR_POOL_SIZE as usize,
                        args,
                    )
                    .await?;
                    Ok(Self { ctx, engine, $($component),* })
                }

                pub fn context(&self) -> &$crate::base::EngineContext {
                    &self.ctx
                }

                pub fn inner(&self) -> &$crate::command::CommandEngine<$store, $domain> {
                    &self.engine
                }
            }
        }

        $crate::command_engine!(@domain_trait_impl_fixed $engine, $($domain_trait)?);
        impl $engine {
            $crate::command_engine!(@methods_body $domain, { $($commands)* }, { $($methods)* });
        }
    };

    // --- full: engine + commands + methods ---
    (
        engine $engine:ident {
            $(domain_trait: $domain_trait:ident,)?
            $(
                meta {
                    $($pool_key:ident : $pool_size:literal),+ $(,)?
                }
            )?
            aggregate: $domain:ident,
            component: {
                $($component:ident : $component_ty:ty),* $(,)?
            } $(,)?
        }
        commands {
            $($commands:tt)*
        }
        methods (
            $($methods:tt)*
        )
    ) => {
        $crate::command_engine!(@commands_body $domain, { $($commands)* });

        $crate::paste::paste! {
            pub struct $engine<S: $crate::datastore::DataStore + 'static> {
                ctx: $crate::base::EngineContext,
                engine: $crate::command::CommandEngine<S, $domain<S>>,
                $(
                    #[allow(dead_code)]
                    $component: $component_ty,
                )*
            }

            impl<S: $crate::datastore::DataStore + 'static> Clone for $engine<S> {
                fn clone(&self) -> Self {
                    Self {
                        ctx: self.ctx.clone(),
                        engine: self.engine.clone(),
                        $($component: self.$component.clone(),)*
                    }
                }
            }

            impl<S: $crate::datastore::DataStore + 'static> $engine<S> {
                /// Concurrent command actors for this domain engine (from engine meta, default 4).
                /// Prefer meta key `command_pool_size`; `actor_pool_size` remains a compatibility alias.
                pub const ACTOR_POOL_SIZE: u32 = $crate::command_engine!(@pool_size $($($pool_key = $pool_size),+)?);
                pub const COMMAND_POOL_SIZE: u32 = Self::ACTOR_POOL_SIZE;

                pub async fn spawn(
                    ctx: $crate::base::EngineContext,
                    logstore: $crate::logstore::DomainLogStore,
                    $($component: $component_ty,)*
                    statemgr: ::std::sync::Arc<S>,
                ) -> $crate::base::RiverbaseResult<Self> {
                    let mut args = $crate::command::CommandEngineArgs::new(
                        logstore,
                        $($component.clone(),)*
                        statemgr,
                    );
                    $crate::command_engine!(@register_handlers $domain, { $($commands)* }, args);
                    args.apply_engine_context(&ctx);
                    let engine = $crate::command::CommandEngine::spawn_with_size(
                        Self::ACTOR_POOL_SIZE as usize,
                        args,
                    )
                    .await?;
                    Ok(Self { ctx, engine, $($component),* })
                }

                /// Spawn with no command handlers (query-only portals).
                pub async fn spawn_empty(
                    ctx: $crate::base::EngineContext,
                    logstore: $crate::logstore::DomainLogStore,
                    $($component: $component_ty,)*
                    statemgr: ::std::sync::Arc<S>,
                ) -> $crate::base::RiverbaseResult<Self> {
                    let mut args = $crate::command::CommandEngineArgs::new(
                        logstore,
                        $($component.clone(),)*
                        statemgr,
                    );
                    args.apply_engine_context(&ctx);
                    let engine = $crate::command::CommandEngine::spawn_with_size(
                        Self::ACTOR_POOL_SIZE as usize,
                        args,
                    )
                    .await?;
                    Ok(Self { ctx, engine, $($component),* })
                }

                pub fn context(&self) -> &$crate::base::EngineContext {
                    &self.ctx
                }

                pub fn inner(&self) -> &$crate::command::CommandEngine<S, $domain<S>> {
                    &self.engine
                }

                pub async fn invoke<H>(
                    &self,
                    payload: H::Payload,
                    target: Option<$crate::command::CommandTarget>,
                ) -> $crate::base::RiverbaseResult<::serde_json::Value>
                where
                    H: $crate::command::TypedCommandHandler<$domain<S>> + Default + 'static,
                {
                    self.engine.invoke::<H>(&self.ctx, payload, target).await
                }
            }
        }

        $crate::command_engine!(@domain_trait_impl $engine, $($domain_trait)?);
        $crate::command_engine!(@engine_methods_impl $engine, $domain, { $($commands)* }, { $($methods)* });
    };

    // Default: generate DomainCommandEngine that delegates to `self.engine`.
    (@domain_trait_impl $engine:ident,) => {
        $crate::impl_domain_command_engine!($engine);
    };
    (@domain_trait_impl $engine:ident, generate) => {
        $crate::impl_domain_command_engine!($engine);
    };
    (@domain_trait_impl $engine:ident, manual) => {};
    (@domain_trait_impl $engine:ident, $unknown:ident) => {
        compile_error!(concat!(
            "command_engine!: unknown domain_trait `",
            stringify!($unknown),
            "` (expected `generate` or `manual`)"
        ));
    };

    (@domain_trait_impl_fixed $engine:ident,) => {
        $crate::impl_domain_command_engine!($engine without_store);
    };
    (@domain_trait_impl_fixed $engine:ident, generate) => {
        $crate::impl_domain_command_engine!($engine without_store);
    };
    (@domain_trait_impl_fixed $engine:ident, manual) => {};
    (@domain_trait_impl_fixed $engine:ident, $unknown:ident) => {
        compile_error!(concat!(
            "command_engine!: unknown domain_trait `",
            stringify!($unknown),
            "` (expected `generate` or `manual`)"
        ));
    };

    (@pool_size) => {
        $crate::pool::clamp_actor_pool_size($crate::pool::DEFAULT_ACTOR_POOL_SIZE)
    };
    (@pool_size command_pool_size = $size:literal $(, $($rest:tt)*)?) => {
        $crate::pool::clamp_actor_pool_size($size)
    };
    (@pool_size actor_pool_size = $size:literal $(, $($rest:tt)*)?) => {
        $crate::pool::clamp_actor_pool_size($size)
    };
    (@pool_size $bad:ident = $size:literal $(, $($rest:tt)*)?) => {
        compile_error!(concat!(
            "command_engine!: unknown pool meta key `",
            stringify!($bad),
            "` (expected `command_pool_size` or `actor_pool_size`)"
        ))
    };

    (@engine_methods_impl $engine:ident, $domain:ident, { $($commands:tt)* }, { $($methods:tt)* }) => {
        impl<S: $crate::datastore::DataStore + 'static> $engine<S> {
            $crate::command_engine!(@methods_body $domain, { $($commands)* }, { $($methods)* });
        }
    };

    // --- handler expansion (@commands_body) ---
    (@commands_body $domain:ident, { }) => {};

    (@commands_body $domain:ident, {
        command $cmd_name:ident {
            meta {
                key: $key:literal,
                title: $title:literal,
                kind: $kind:ident,
                resources: [$($resource:expr),* $(,)?],
                $($meta_extra:tt)*
            }
            payload {
                $(
                    $(#[$field_meta:meta])*
                    $field_name:ident : $field_ty:path,
                )*
            }
            target collection($target_coll:expr);
            handle ($payload:ident, $aggregate:ident) $body:block
        }
        $($tail:tt)*
    }) => {
        $crate::command_engine!(@commands_body_emit
            $domain,
            $cmd_name,
            $kind,
            $key,
            $title,
            [$($resource),*],
            { $($meta_extra)* },
            [ $($field_name)* ],
            {
                $(
                    $(#[$field_meta])*
                        pub $field_name: $field_ty,
                )*
            },
            $payload,
            $aggregate,
            $body,
            collection($target_coll)
        );
        $crate::command_engine!(@commands_body $domain, { $($tail)* });
    };

    (@commands_body $domain:ident, {
        command $cmd_name:ident {
            meta {
                key: $key:literal,
                title: $title:literal,
                kind: $kind:ident,
                resources: [$($resource:expr),* $(,)?],
                $($meta_extra:tt)*
            }
            payload {
                $(
                    $(#[$field_meta:meta])*
                    $field_name:ident : $field_ty:path,
                )*
            }
            target object($target_obj:expr);
            handle ($payload:ident, $aggregate:ident) $body:block
        }
        $($tail:tt)*
    }) => {
        $crate::command_engine!(@commands_body_emit
            $domain,
            $cmd_name,
            $kind,
            $key,
            $title,
            [$($resource),*],
            { $($meta_extra)* },
            [ $($field_name)* ],
            {
                $(
                    $(#[$field_meta])*
                        pub $field_name: $field_ty,
                )*
            },
            $payload,
            $aggregate,
            $body,
            object($target_obj)
        );
        $crate::command_engine!(@commands_body $domain, { $($tail)* });
    };

    (@commands_body $domain:ident, {
        command $cmd_name:ident {
            meta {
                key: $key:literal,
                title: $title:literal,
                kind: $kind:ident,
                resources: [$($resource:expr),* $(,)?],
                $($meta_extra:tt)*
            }
            payload {
                $(
                    $(#[$field_meta:meta])*
                    $field_name:ident : $field_ty:path,
                )*
            }
            handle ($payload:ident, $aggregate:ident) $body:block
        }
        $($tail:tt)*
    }) => {
        $crate::command_engine!(@commands_body_emit
            $domain,
            $cmd_name,
            $kind,
            $key,
            $title,
            [$($resource),*],
            { $($meta_extra)* },
            [ $($field_name)* ],
            {
                $(
                    $(#[$field_meta])*
                        pub $field_name: $field_ty,
                )*
            },
            $payload,
            $aggregate,
            $body,
            none
        );
        $crate::command_engine!(@commands_body $domain, { $($tail)* });
    };

    (@commands_body $domain:ident, {
        command $cmd_name:ident {
            meta {
                key: $key:literal,
                title: $title:literal,
                kind: $kind:ident,
                resources: [$($resource:expr),* $(,)?],
                $($meta_extra:tt)*
            }
            payload {}
            target collection($target_coll:expr);
            handle ($payload:ident, $aggregate:ident) $body:block
        }
        $($tail:tt)*
    }) => {
        $crate::command_engine!(@commands_body_emit
            $domain,
            $cmd_name,
            $kind,
            $key,
            $title,
            [$($resource),*],
            { $($meta_extra)* },
            [],
            {},
            $payload,
            $aggregate,
            $body,
            collection($target_coll)
        );
        $crate::command_engine!(@commands_body $domain, { $($tail)* });
    };

    (@commands_body $domain:ident, {
        command $cmd_name:ident {
            meta {
                key: $key:literal,
                title: $title:literal,
                kind: $kind:ident,
                resources: [$($resource:expr),* $(,)?],
                $($meta_extra:tt)*
            }
            payload {}
            handle ($payload:ident, $aggregate:ident) $body:block
        }
        $($tail:tt)*
    }) => {
        $crate::command_engine!(@commands_body_emit
            $domain,
            $cmd_name,
            $kind,
            $key,
            $title,
            [$($resource),*],
            { $($meta_extra)* },
            [],
            {},
            $payload,
            $aggregate,
            $body,
            none
        );
        $crate::command_engine!(@commands_body $domain, { $($tail)* });
    };

    (@commands_body_fixed $domain:ident, { }) => {};

    (@commands_body_fixed $domain:ident, {
        command $cmd_name:ident {
            meta {
                key: $key:literal,
                title: $title:literal,
                kind: $kind:ident,
                resources: [$($resource:expr),* $(,)?],
                $($meta_extra:tt)*
            }
            payload {
                $(
                    $(#[$field_meta:meta])*
                    $field_name:ident : $field_ty:path,
                )*
            }
            target collection($target_coll:expr);
            handle ($payload:ident, $aggregate:ident) $body:block
        }
        $($tail:tt)*
    }) => {
        $crate::command_engine!(@commands_body_emit_fixed
            $domain,
            $cmd_name,
            $kind,
            $key,
            $title,
            [$($resource),*],
            { $($meta_extra)* },
            [ $($field_name)* ],
            {
                $(
                    $(#[$field_meta])*
                        pub $field_name: $field_ty,
                )*
            },
            $payload,
            $aggregate,
            $body,
            collection($target_coll)
        );
        $crate::command_engine!(@commands_body_fixed $domain, { $($tail)* });
    };

    (@commands_body_fixed $domain:ident, {
        command $cmd_name:ident {
            meta {
                key: $key:literal,
                title: $title:literal,
                kind: $kind:ident,
                resources: [$($resource:expr),* $(,)?],
                $($meta_extra:tt)*
            }
            payload {
                $(
                    $(#[$field_meta:meta])*
                    $field_name:ident : $field_ty:path,
                )*
            }
            handle ($payload:ident, $aggregate:ident) $body:block
        }
        $($tail:tt)*
    }) => {
        $crate::command_engine!(@commands_body_emit_fixed
            $domain,
            $cmd_name,
            $kind,
            $key,
            $title,
            [$($resource),*],
            { $($meta_extra)* },
            [ $($field_name)* ],
            {
                $(
                    $(#[$field_meta])*
                        pub $field_name: $field_ty,
                )*
            },
            $payload,
            $aggregate,
            $body,
            none
        );
        $crate::command_engine!(@commands_body_fixed $domain, { $($tail)* });
    };

    (@commands_body_fixed $domain:ident, {
        $bad:tt $($rest:tt)*
    }) => {
        compile_error!(concat!(
            "command_engine!: unsupported command form in `store:` engine near `",
            stringify!($bad),
            "`"
        ));
    };

    (@commands_body_emit
        $domain:ident,
        $cmd_name:ident,
        $kind:ident,
        $key:literal,
        $title:literal,
        [$($resource:expr),*],
        { $($meta_extra:tt)* },
        [ $($field_name:ident)* ],
        { $($payload_fields:tt)* },
        $payload:ident,
        $aggregate:ident,
        $body:block,
        collection($target_coll:expr)
    ) => {
        $crate::paste::paste! {
            $crate::command_engine!(@emit_payload_fields
                [ $($field_name)* ],
                [<$cmd_name Payload>],
                { $($payload_fields)* }
            );
            #[derive(Default)]
            pub struct [<$cmd_name Handler>];

            impl [<$cmd_name Handler>] {
                async fn run<S: $crate::datastore::DataStore>(
                    $payload: [<$cmd_name Payload>],
                    $aggregate: &mut $domain<S>,
                ) -> $crate::base::RiverbaseResult<::serde_json::Value> {
                    $body
                }
            }

            #[::async_trait::async_trait]
            impl<S: $crate::datastore::DataStore> $crate::command::TypedCommandHandler<$domain<S>> for [<$cmd_name Handler>] {
                type Payload = [<$cmd_name Payload>];

                fn meta(&self) -> $crate::command::CommandMeta {
                    $crate::command_engine!(@meta_finish
                        ($crate::command_engine!(@meta_base
                            $kind,
                            $key,
                            $title,
                            [$($resource),*]
                        ))
                        $($meta_extra)*
                    )
                }

                fn target(&self, _: &Self::Payload) -> Option<$crate::command::CommandTarget> {
                    Some($crate::command::CommandTarget::collection($target_coll))
                }

                async fn handle(
                    &self,
                    payload: Self::Payload,
                    aggregate: &mut $domain<S>,
                ) -> $crate::base::RiverbaseResult<::serde_json::Value> {
                    Self::run::<S>(payload, aggregate).await
                }
            }
        }
    };

    (@commands_body_emit
        $domain:ident,
        $cmd_name:ident,
        $kind:ident,
        $key:literal,
        $title:literal,
        [$($resource:expr),*],
        { $($meta_extra:tt)* },
        [ $($field_name:ident)* ],
        { $($payload_fields:tt)* },
        $payload:ident,
        $aggregate:ident,
        $body:block,
        object($target_obj:expr)
    ) => {
        $crate::paste::paste! {
            $crate::command_engine!(@emit_payload_fields
                [ $($field_name)* ],
                [<$cmd_name Payload>],
                { $($payload_fields)* }
            );
            #[derive(Default)]
            pub struct [<$cmd_name Handler>];

            impl [<$cmd_name Handler>] {
                async fn run<S: $crate::datastore::DataStore>(
                    $payload: [<$cmd_name Payload>],
                    $aggregate: &mut $domain<S>,
                ) -> $crate::base::RiverbaseResult<::serde_json::Value> {
                    $body
                }
            }

            #[::async_trait::async_trait]
            impl<S: $crate::datastore::DataStore> $crate::command::TypedCommandHandler<$domain<S>> for [<$cmd_name Handler>] {
                type Payload = [<$cmd_name Payload>];

                fn meta(&self) -> $crate::command::CommandMeta {
                    $crate::command_engine!(@meta_finish
                        ($crate::command_engine!(@meta_base
                            $kind,
                            $key,
                            $title,
                            [$($resource),*]
                        ))
                        $($meta_extra)*
                    )
                }

                fn target(&self, _: &Self::Payload) -> Option<$crate::command::CommandTarget> {
                    Some($crate::command::CommandTarget::object($target_obj))
                }

                async fn handle(
                    &self,
                    payload: Self::Payload,
                    aggregate: &mut $domain<S>,
                ) -> $crate::base::RiverbaseResult<::serde_json::Value> {
                    Self::run::<S>(payload, aggregate).await
                }
            }
        }
    };

    (@commands_body_emit
        $domain:ident,
        $cmd_name:ident,
        $kind:ident,
        $key:literal,
        $title:literal,
        [$($resource:expr),*],
        { $($meta_extra:tt)* },
        [ $($field_name:ident)* ],
        { $($payload_fields:tt)* },
        $payload:ident,
        $aggregate:ident,
        $body:block,
        none
    ) => {
        $crate::paste::paste! {
            $crate::command_engine!(@emit_payload_fields
                [ $($field_name)* ],
                [<$cmd_name Payload>],
                { $($payload_fields)* }
            );
            #[derive(Default)]
            pub struct [<$cmd_name Handler>];

            impl [<$cmd_name Handler>] {
                async fn run<S: $crate::datastore::DataStore>(
                    $payload: [<$cmd_name Payload>],
                    $aggregate: &mut $domain<S>,
                ) -> $crate::base::RiverbaseResult<::serde_json::Value> {
                    $body
                }
            }

            #[::async_trait::async_trait]
            impl<S: $crate::datastore::DataStore> $crate::command::TypedCommandHandler<$domain<S>> for [<$cmd_name Handler>] {
                type Payload = [<$cmd_name Payload>];

                fn meta(&self) -> $crate::command::CommandMeta {
                    $crate::command_engine!(@meta_finish
                        ($crate::command_engine!(@meta_base
                            $kind,
                            $key,
                            $title,
                            [$($resource),*]
                        ))
                        $($meta_extra)*
                    )
                }

                async fn handle(
                    &self,
                    payload: Self::Payload,
                    aggregate: &mut $domain<S>,
                ) -> $crate::base::RiverbaseResult<::serde_json::Value> {
                    Self::run::<S>(payload, aggregate).await
                }
            }
        }
    };

    (@commands_body_emit_fixed
        $domain:ident,
        $cmd_name:ident,
        $kind:ident,
        $key:literal,
        $title:literal,
        [$($resource:expr),*],
        { $($meta_extra:tt)* },
        [ $($field_name:ident)* ],
        { $($payload_fields:tt)* },
        $payload:ident,
        $aggregate:ident,
        $body:block,
        collection($target_coll:expr)
    ) => {
        $crate::paste::paste! {
            $crate::command_engine!(@emit_payload_fields
                [ $($field_name)* ],
                [<$cmd_name Payload>],
                { $($payload_fields)* }
            );
            #[derive(Default)]
            pub struct [<$cmd_name Handler>];

            impl [<$cmd_name Handler>] {
                async fn run(
                    $payload: [<$cmd_name Payload>],
                    $aggregate: &mut $domain,
                ) -> $crate::base::RiverbaseResult<::serde_json::Value> {
                    $body
                }
            }

            #[::async_trait::async_trait]
            impl $crate::command::TypedCommandHandler<$domain> for [<$cmd_name Handler>] {
                type Payload = [<$cmd_name Payload>];

                fn meta(&self) -> $crate::command::CommandMeta {
                    $crate::command_engine!(@meta_finish
                        ($crate::command_engine!(@meta_base
                            $kind,
                            $key,
                            $title,
                            [$($resource),*]
                        ))
                        $($meta_extra)*
                    )
                }

                fn target(&self, _: &Self::Payload) -> Option<$crate::command::CommandTarget> {
                    Some($crate::command::CommandTarget::collection($target_coll))
                }

                async fn handle(
                    &self,
                    payload: Self::Payload,
                    aggregate: &mut $domain,
                ) -> $crate::base::RiverbaseResult<::serde_json::Value> {
                    Self::run(payload, aggregate).await
                }
            }
        }
    };

    (@commands_body_emit_fixed
        $domain:ident,
        $cmd_name:ident,
        $kind:ident,
        $key:literal,
        $title:literal,
        [$($resource:expr),*],
        { $($meta_extra:tt)* },
        [ $($field_name:ident)* ],
        { $($payload_fields:tt)* },
        $payload:ident,
        $aggregate:ident,
        $body:block,
        none
    ) => {
        $crate::paste::paste! {
            $crate::command_engine!(@emit_payload_fields
                [ $($field_name)* ],
                [<$cmd_name Payload>],
                { $($payload_fields)* }
            );
            #[derive(Default)]
            pub struct [<$cmd_name Handler>];

            impl [<$cmd_name Handler>] {
                async fn run(
                    $payload: [<$cmd_name Payload>],
                    $aggregate: &mut $domain,
                ) -> $crate::base::RiverbaseResult<::serde_json::Value> {
                    $body
                }
            }

            #[::async_trait::async_trait]
            impl $crate::command::TypedCommandHandler<$domain> for [<$cmd_name Handler>] {
                type Payload = [<$cmd_name Payload>];

                fn meta(&self) -> $crate::command::CommandMeta {
                    $crate::command_engine!(@meta_finish
                        ($crate::command_engine!(@meta_base
                            $kind,
                            $key,
                            $title,
                            [$($resource),*]
                        ))
                        $($meta_extra)*
                    )
                }

                async fn handle(
                    &self,
                    payload: Self::Payload,
                    aggregate: &mut $domain,
                ) -> $crate::base::RiverbaseResult<::serde_json::Value> {
                    Self::run(payload, aggregate).await
                }
            }
        }
    };

    (@emit_payload_fields
        [],
        $payload:ident,
        { $($payload_fields:tt)* }
    ) => {
        #[derive(
            Debug,
            ::serde::Deserialize,
            ::serde::Serialize,
            Default,
            $crate::garde::Validate,
            $crate::schemars::JsonSchema,
        )]
        #[garde(allow_unvalidated)]
        pub struct $payload {
            $($payload_fields)*
        }
    };

    (@emit_payload_fields
        [$($_field:ident)+],
        $payload:ident,
        { $($payload_fields:tt)* }
    ) => {
        #[derive(
            Debug,
            ::serde::Deserialize,
            ::serde::Serialize,
            $crate::garde::Validate,
            $crate::schemars::JsonSchema,
        )]
        #[garde(allow_unvalidated)]
        pub struct $payload {
            $($payload_fields)*
        }
    };

    (@meta_base collection, $key:literal, $title:literal, [$($resource:expr),*]) => {
        $crate::command::CommandMeta::collection($key, $title)
            .with_resources([$($resource),*])
            .with_engine_method(true)
    };

    (@meta_base object, $key:literal, $title:literal, [$($resource:expr),*]) => {
        $crate::command::CommandMeta::object($key, $title)
            .with_resources([$($resource),*])
            .with_engine_method(true)
    };

    (@meta_finish ($meta:expr)) => {
        $meta
    };

    (@meta_finish ($meta:expr) engine_method: false $($tail:tt)*) => {
        $crate::command_engine!(@meta_finish
            ({
                let meta = $meta;
                meta.with_engine_method(false)
            })
            $($tail)*
        )
    };

    (@meta_finish ($meta:expr) engine_method: true $($tail:tt)*) => {
        $crate::command_engine!(@meta_finish ($meta) $($tail)*)
    };

    (@meta_finish ($meta:expr) scope: required $($tail:tt)*) => {
        $crate::command_engine!(@meta_finish
            ({
                let meta = $meta;
                meta.with_scope($crate::base::ScopeMeta::required(None))
            })
            $($tail)*
        )
    };

    (@meta_finish ($meta:expr) hook: true $($tail:tt)*) => {
        $crate::command_engine!(@meta_finish
            ({
                let meta = $meta;
                meta.with_hook_method()
            })
            $($tail)*
        )
    };

    (@meta_finish ($meta:expr) link: true $($tail:tt)*) => {
        $crate::command_engine!(@meta_finish
            ({
                let meta = $meta;
                meta.with_link_method()
            })
            $($tail)*
        )
    };

    (@meta_finish ($meta:expr) openapi_tag: $tag:literal $($tail:tt)*) => {
        $crate::command_engine!(@meta_finish
            ({
                let meta = $meta;
                meta.with_openapi_tag($tag)
            })
            $($tail)*
        )
    };

    (@meta_finish ($meta:expr) openapi_explorer: $explorer:ident $($tail:tt)*) => {
        $crate::command_engine!(@meta_finish
            ({
                let meta = $meta;
                meta.with_openapi_explorer($crate::command_engine!(@openapi_bool $explorer))
            })
            $($tail)*
        )
    };

    (@meta_finish ($meta:expr) openapi_internal: $internal:ident $($tail:tt)*) => {
        $crate::command_engine!(@meta_finish
            ({
                let meta = $meta;
                meta.with_openapi_internal($crate::command_engine!(@openapi_bool $internal))
            })
            $($tail)*
        )
    };

    (@meta_finish ($meta:expr) allowed_zones: [ $($zone:literal),* $(,)? ] $($tail:tt)*) => {
        $crate::command_engine!(@meta_finish
            ({
                let meta = $meta;
                meta.with_allowed_zones([$($zone),*])
            })
            $($tail)*
        )
    };

    (@meta_finish ($meta:expr) roles_required: [ $($role:literal),* $(,)? ] $($tail:tt)*) => {
        $crate::command_engine!(@meta_finish
            ({
                let meta = $meta;
                meta.with_roles_required([$($role),*])
            })
            $($tail)*
        )
    };

    (@meta_finish ($meta:expr) tenant_scope: exempt ( $reason:literal , $signoff:literal ) $($tail:tt)*) => {
        $crate::command_engine!(@meta_finish
            ({
                let meta = $meta;
                meta.with_tenant_scope_exempt($reason, $signoff)
            })
            $($tail)*
        )
    };

    (@meta_finish ($meta:expr) response_type: $response_type:literal $($tail:tt)*) => {
        $crate::command_engine!(@meta_finish
            ({
                let meta = $meta;
                meta.with_response_type($response_type)
            })
            $($tail)*
        )
    };

    (@openapi_bool true) => { true };
    (@openapi_bool false) => { false };

    (@meta_finish ($meta:expr) , $($tail:tt)*) => {
        $crate::command_engine!(@meta_finish ($meta) $($tail)*)
    };

    (@meta_finish ($meta:expr) $unknown:tt $($tail:tt)*) => {
        compile_error!(concat!(
            "command_engine!: unknown meta option `",
            stringify!($unknown),
            "`"
        ));
    };

    // --- registration ---
    (@register_handlers $domain:ident, { $($commands:tt)* }, $args:expr) => {
        $crate::command_engine!(@register_handlers_inner $domain, { $($commands)* }, $args);
    };

    (@register_handlers_inner $domain:ident, { }, $args:expr) => {};

    (@register_handlers_inner $domain:ident, {
        command $cmd_name:ident { $($inner:tt)* }
        $($tail:tt)*
    }, $args:expr) => {
        $crate::paste::paste! {
            $args.register_typed([<$cmd_name Handler>]::default());
            $crate::command_engine!(@register_handlers_inner $domain, { $($tail)* }, $args);
        }
    };

    (@register_fn_hidden $domain:ident, { $($commands:tt)* }) => {
        $crate::paste::paste! {
            #[doc(hidden)]
            pub fn [<register_ $domain:snake _handlers>]<S: $crate::datastore::DataStore + 'static>(
                args: &mut $crate::command::CommandEngineArgs<S, $domain<S>>,
            ) {
                $crate::command_engine!(@register_handlers $domain, { $($commands)* }, args);
            }
        }
    };

    (@engine_args_new $logstore:expr, $msgbus:expr, $statemgr:expr) => {
        $crate::command::CommandEngineArgs::new($logstore, $msgbus.clone(), $statemgr)
    };

    (@engine_args_new $logstore:expr, $statemgr:expr) => {
        compile_error!("command_engine!: component `msgbus` is required in `component: { msgbus: ... }`");
    };

    // --- methods: each entry declares its payload + target explicitly ---
    (@methods_body $domain:ident, { $($commands:tt)* }, { }) => {};

    (@methods_body $domain:ident, { $($commands:tt)* }, {
        $meth:ident ( $( $param:ident : [ $($param_ty:tt)* ] );* $(;)? ) for $cmd:ident {
            payload $payload:expr,
            target none $(,)?
        }
        $($tail:tt)*
    }) => {
        $crate::paste::paste! {
            pub async fn $meth(
                &self,
                $( $param : $($param_ty)* ),*
            ) -> $crate::base::RiverbaseResult<::serde_json::Value> {
                self.engine.invoke::<[<$cmd Handler>]>(&self.ctx, $payload, None).await
            }
        }
        $crate::command_engine!(@methods_body $domain, { $($commands)* }, { $($tail)* });
    };

    (@methods_body $domain:ident, { $($commands:tt)* }, {
        $meth:ident ( $( $param:ident : [ $($param_ty:tt)* ] );* $(;)? ) for $cmd:ident {
            payload $payload:expr,
            target some($target:expr) $(,)?
        }
        $($tail:tt)*
    }) => {
        $crate::paste::paste! {
            pub async fn $meth(
                &self,
                $( $param : $($param_ty)* ),*
            ) -> $crate::base::RiverbaseResult<::serde_json::Value> {
                self.engine.invoke::<[<$cmd Handler>]>(&self.ctx, $payload, Some($target)).await
            }
        }
        $crate::command_engine!(@methods_body $domain, { $($commands)* }, { $($tail)* });
    };

    (@methods_body $domain:ident, { $($commands:tt)* }, {
        $bad:tt $($rest:tt)*
    }) => {
        compile_error!(concat!(
            "command_engine!: invalid methods entry near `",
            stringify!($bad),
            "` — expected `name(params...) for CommandName { payload ...; target none|some(...) }`"
        ));
    };

    // Catch common top-level mistakes early with a readable message.
    (
        engine $engine:ident {
            $($inner:tt)*
        }
    ) => {
        compile_error!(concat!(
            "command_engine!: engine `",
            stringify!($engine),
            "` requires `commands { ... }` and `methods ( ... )` blocks after the engine meta"
        ));
    };
}

/// Alias for [`command_engine!`]. Mirrors the `query_resource!` / `query_engine!` naming.
#[macro_export]
macro_rules! command {
    ($($tt:tt)*) => {
        $crate::command_engine!($($tt)*);
    };
}

/// Generate a forwarding [`DomainCommandEngine`](crate::domain::DomainCommandEngine) impl for a
/// hand-written engine that stores `ctx: EngineContext` and `engine: CommandEngine<...>`.
///
/// Prefer the full `command_engine! { engine ... }` form (which emits this automatically). Use
/// this helper for commands-only macros that still define the engine struct by hand.
///
/// ```ignore
/// riverbase_core::impl_domain_command_engine!(PricingCommandEngine);
/// // non-generic engine:
/// riverbase_core::impl_domain_command_engine!(SettingCommandEngine without_store);
/// ```
#[macro_export]
macro_rules! impl_domain_command_engine {
    ($engine:ident) => {
        #[::async_trait::async_trait]
        impl<S: $crate::datastore::DataStore + 'static> $crate::domain::DomainCommandEngine
            for $engine<S>
        {
            fn context(&self) -> &$crate::base::EngineContext {
                &self.ctx
            }

            async fn commands(
                &self,
            ) -> $crate::base::RiverbaseResult<::std::vec::Vec<::std::string::String>> {
                $crate::base::Engine::items(&self.engine).await
            }

            fn command_meta(
                &self,
                cmdkey: &str,
            ) -> ::std::option::Option<$crate::command::CommandMeta> {
                self.engine.command_meta(cmdkey)
            }

            fn command_info(&self, cmdkey: &str) -> ::std::option::Option<::serde_json::Value> {
                self.engine.command_info(cmdkey)
            }

            async fn execute(
                &self,
                ctx: &$crate::base::EngineContext,
                cmdkey: &str,
                payload: ::serde_json::Value,
                target: $crate::command::CommandTarget,
            ) -> $crate::base::RiverbaseResult<::serde_json::Value> {
                self.engine.execute(ctx, cmdkey, payload, target).await
            }

            async fn execute_prepared_batch(
                &self,
                ctx: &$crate::base::EngineContext,
                items: ::std::vec::Vec<$crate::command::PreparedCommand>,
            ) -> $crate::base::RiverbaseResult<$crate::command::BatchExecuteResult> {
                self.engine.execute_prepared_batch(ctx, items).await
            }
        }
    };

    ($engine:ident without_store) => {
        #[::async_trait::async_trait]
        impl $crate::domain::DomainCommandEngine for $engine {
            fn context(&self) -> &$crate::base::EngineContext {
                &self.ctx
            }

            async fn commands(
                &self,
            ) -> $crate::base::RiverbaseResult<::std::vec::Vec<::std::string::String>> {
                $crate::base::Engine::items(&self.engine).await
            }

            fn command_meta(
                &self,
                cmdkey: &str,
            ) -> ::std::option::Option<$crate::command::CommandMeta> {
                self.engine.command_meta(cmdkey)
            }

            fn command_info(&self, cmdkey: &str) -> ::std::option::Option<::serde_json::Value> {
                self.engine.command_info(cmdkey)
            }

            async fn execute(
                &self,
                ctx: &$crate::base::EngineContext,
                cmdkey: &str,
                payload: ::serde_json::Value,
                target: $crate::command::CommandTarget,
            ) -> $crate::base::RiverbaseResult<::serde_json::Value> {
                self.engine.execute(ctx, cmdkey, payload, target).await
            }

            async fn execute_prepared_batch(
                &self,
                ctx: &$crate::base::EngineContext,
                items: ::std::vec::Vec<$crate::command::PreparedCommand>,
            ) -> $crate::base::RiverbaseResult<$crate::command::BatchExecuteResult> {
                self.engine.execute_prepared_batch(ctx, items).await
            }
        }
    };
}
