//! `domain!` — generate a concrete domain type that implements [`Domain`](crate::domain::Domain).
//!
//! The generated domain is generic over its [`DataStore`](crate::datastore::DataStore): the
//! command engine is parameterized as `$command<S>`, while the query/service engines stay
//! object-safe (they erase the concrete store behind `DataStore`). The macro emits the struct,
//! the [`Domain`] trait impl, compile-time identity consts, and helpers (`from_engines`,
//! `into_arc`, `command()`/`query()`/`service()`, `execute_command`, `execute_query`).
//!
//! An optional `spawn(...) { ... }` block declares the constructor. It is emitted inside
//! `impl<S: DataStoreInit> $domain<S>`, so its body can refer to `S` (e.g. `S::init(...)`).
//!
//! An optional `routes(api_base) { ... }` block declares custom HTTP endpoints (uploads,
//! downloads, websockets, …) that do not fit the generated command/query route model. The block
//! is emitted as a [`riverbase_http::WebDomain`](riverbase_http::WebDomain) implementation; its body
//! has access to `self` and the `api_base` binding (`&str`) and must evaluate to an
//! [`axum::Router`](riverbase_http::axum::Router). Use `riverbase_http::axum` so the domain crate
//! depends on `riverbase_http` only for custom routes. The router is merged alongside the coupled
//! app's mount layer.
//!
//! Query-only domains may omit `command:` entirely; the macro defaults to
//! [`DisabledCommandEngine`](crate::domain::DisabledCommandEngine) (fully qualified —
//! no import required) and `capabilities { command: false, query: true }`.
//! Prefer `Self::from_query(ctx, query)` in spawn.
//!
//! A `routes(api_base) { ... }` block requires `riverbase_core` feature `http` and a
//! `riverbase_http` dependency. Kernel-only domain crates omit the block.
//!
//! ```ignore
//! riverbase_core::domain! {
//!     domain TodoDomain {
//!         meta { namespace: "riverbase.todo", title: "Todo", description: "…" }
//!         command: TodoCommandEngine,
//!         query: TodoQueryEngine,
//!         service: TodoServiceEngine,
//!         routes(api_base) {
//!             use riverbase_http::axum::{routing::get, Router};
//!             let path = format!("{api_base}/{}/health~probe", Self::NAMESPACE);
//!             Router::new().route(&path, get(|| async { "ok" }))
//!         }
//!         spawn(runtime: &DomainRuntime) {
//!             let ctx = EngineContext::new(Self::NAMESPACE);
//!             let store = S::init(&runtime.dbconn, todo_store_spec())?;
//!             let command = Arc::new(TodoCommandEngine::spawn(
//!                 ctx.clone(), runtime.logstore.clone(), runtime.msgbus.clone(), store.clone(),
//!             ).await?);
//!             let query = Arc::new(TodoQueryEngine::spawn(ctx.clone(), store.clone(), runtime.logstore.clone()).await?);
//!             let service = Arc::new(TodoServiceEngine::spawn(ctx, runtime.logstore.clone(), runtime.msgbus.clone(), store).await?);
//!             Ok(Self::from_engines(command, query, service))
//!         }
//!     }
//! }
//! ```
//!
//! Domains needing extra constructors (e.g. `rfx_idm::spawn_queries_only`) add them in a separate
//! generic `impl<S: ...>` block alongside the macro invocation.
//!
//! [`EngineContext`]: crate::base::EngineContext
/// Compose a domain type that implements [`Domain`](crate::domain::Domain).
#[macro_export]
macro_rules! domain {
    // ---- With spawn, description -------------------------------------------------------------
    (
        domain $domain:ident {
            meta {
                namespace: $namespace:expr,
                title: $title:expr,
                description: $description:expr $(,)?
            }
            $(command: $command:ident,)?
            query: $query:ty,
            $( capabilities { command: $command_capability:tt, query: $query_capability:tt } )?
            $( service: $service:ty, )?
            $( routes ( $routes_param:ident ) { $( $routes_body:tt )* } )?
            spawn (
                $( $param:ident : $param_ty:ty ),* $(,)?
            ) {
                $( $body:tt )*
            }
        }
    ) => {
        $crate::domain!(@resolve_command $domain,
            ($($command)?),
            $query,
            namespace = $namespace, title = $title,
            description = ::core::option::Option::Some($description),
            $( capabilities { command: $command_capability, query: $query_capability } )?
            $( service = $service , )?
            $( routes ( $routes_param ) { $( $routes_body )* } )?
        );
        $crate::domain!(@spawn $domain,
            ( $( $param : $param_ty ),* )
            { $( $body )* }
        );
    };

    // ---- With spawn, no description ----------------------------------------------------------
    (
        domain $domain:ident {
            meta {
                namespace: $namespace:expr,
                title: $title:expr $(,)?
            }
            $(command: $command:ident,)?
            query: $query:ty,
            $( capabilities { command: $command_capability:tt, query: $query_capability:tt } )?
            $( service: $service:ty, )?
            $( routes ( $routes_param:ident ) { $( $routes_body:tt )* } )?
            spawn (
                $( $param:ident : $param_ty:ty ),* $(,)?
            ) {
                $( $body:tt )*
            }
        }
    ) => {
        $crate::domain!(@resolve_command $domain,
            ($($command)?),
            $query,
            namespace = $namespace, title = $title,
            description = ::core::option::Option::<&'static str>::None,
            $( capabilities { command: $command_capability, query: $query_capability } )?
            $( service = $service , )?
            $( routes ( $routes_param ) { $( $routes_body )* } )?
        );
        $crate::domain!(@spawn $domain,
            ( $( $param : $param_ty ),* )
            { $( $body )* }
        );
    };

    // ---- Core only, description --------------------------------------------------------------
    (
        domain $domain:ident {
            meta {
                namespace: $namespace:expr,
                title: $title:expr,
                description: $description:expr $(,)?
            }
            $(command: $command:ident,)?
            query: $query:ty,
            $( capabilities { command: $command_capability:tt, query: $query_capability:tt } )?
            $( service: $service:ty, )?
            $( routes ( $routes_param:ident ) { $( $routes_body:tt )* } )?
        }
    ) => {
        $crate::domain!(@resolve_command $domain,
            ($($command)?),
            $query,
            namespace = $namespace, title = $title,
            description = ::core::option::Option::Some($description),
            $( capabilities { command: $command_capability, query: $query_capability } )?
            $( service = $service , )?
            $( routes ( $routes_param ) { $( $routes_body )* } )?
        );
    };

    // ---- Core only, no description -----------------------------------------------------------
    (
        domain $domain:ident {
            meta {
                namespace: $namespace:expr,
                title: $title:expr $(,)?
            }
            $(command: $command:ident,)?
            query: $query:ty,
            $( capabilities { command: $command_capability:tt, query: $query_capability:tt } )?
            $( service: $service:ty, )?
            $( routes ( $routes_param:ident ) { $( $routes_body:tt )* } )?
        }
    ) => {
        $crate::domain!(@resolve_command $domain,
            ($($command)?),
            $query,
            namespace = $namespace, title = $title,
            description = ::core::option::Option::<&'static str>::None,
            $( capabilities { command: $command_capability, query: $query_capability } )?
            $( service = $service , )?
            $( routes ( $routes_param ) { $( $routes_body )* } )?
        );
    };

    // Omit command → DisabledCommandEngine + default command:false capability.
    (@resolve_command $domain:ident, (), $query:ty,
        namespace = $namespace:expr, title = $title:expr, description = $description:expr,
        $( capabilities { command: $command_capability:tt, query: $query_capability:tt } )?
        $( service = $service:ty , )?
        $( routes ( $routes_param:ident ) { $( $routes_body:tt )* } )?
    ) => {
        $crate::domain!(@core_query_only $domain, $query,
            namespace = $namespace, title = $title, description = $description,
            $( capabilities { command: $command_capability, query: $query_capability } )?
            $( service = $service , )?
            $( routes ( $routes_param ) { $( $routes_body )* } )?
        );
    };

    (@resolve_command $domain:ident, ($command:ident), $query:ty,
        namespace = $namespace:expr, title = $title:expr, description = $description:expr,
        $( capabilities { command: $command_capability:tt, query: $query_capability:tt } )?
        $( service = $service:ty , )?
        $( routes ( $routes_param:ident ) { $( $routes_body:tt )* } )?
    ) => {
        $crate::domain!(@core $domain, $command, $query,
            namespace = $namespace, title = $title, description = $description,
            $( capabilities { command: $command_capability, query: $query_capability } )?
            $( service = $service , )?
            $( routes ( $routes_param ) { $( $routes_body )* } )?
        );
    };

    // ---- spawn constructor (emitted inside impl<S: DataStoreInit>) --------------------------
    (@spawn $domain:ident,
        ( $( $param:ident : $param_ty:ty ),* )
        { $( $body:tt )* }
    ) => {
        impl<S: $crate::domain::DataStoreInit> $domain<S> {
            pub async fn spawn(
                $( $param : $param_ty ),*
            ) -> $crate::base::RiverbaseResult<Self> {
                $( $body )*
            }
        }
    };

    // Query-only with explicit capabilities.
    (@core_query_only $domain:ident, $query:ty,
        namespace = $namespace:expr, title = $title:expr, description = $description:expr,
        capabilities { command: $command_capability:tt, query: $query_capability:tt }
        $( service = $service:ty , )?
        $( routes ( $routes_param:ident ) { $( $routes_body:tt )* } )?
    ) => {
        $crate::paste::paste! {
            type [<$domain __DisabledCommand>]<S> = $crate::domain::DisabledCommandEngine<S>;
            $crate::domain!(@core $domain, [<$domain __DisabledCommand>], $query,
                namespace = $namespace, title = $title, description = $description,
                capabilities { command: $command_capability, query: $query_capability }
                $( service = $service , )?
                $( routes ( $routes_param ) { $( $routes_body )* } )?
            );
        }
        $crate::domain!(@from_query $domain, $query $(, $service)?);
    };

    // Query-only with default capabilities { command: false, query: true }.
    (@core_query_only $domain:ident, $query:ty,
        namespace = $namespace:expr, title = $title:expr, description = $description:expr,
        $( service = $service:ty , )?
        $( routes ( $routes_param:ident ) { $( $routes_body:tt )* } )?
    ) => {
        $crate::paste::paste! {
            type [<$domain __DisabledCommand>]<S> = $crate::domain::DisabledCommandEngine<S>;
            $crate::domain!(@core $domain, [<$domain __DisabledCommand>], $query,
                namespace = $namespace, title = $title, description = $description,
                capabilities { command: false, query: true }
                $( service = $service , )?
                $( routes ( $routes_param ) { $( $routes_body )* } )?
            );
        }
        $crate::domain!(@from_query $domain, $query $(, $service)?);
    };

    (@from_query $domain:ident, $query:ty) => {
        impl<S: $crate::datastore::DataStore + 'static> $domain<S> {
            /// Build a query-only domain with a disabled command slot.
            pub fn from_query(
                ctx: $crate::base::EngineContext,
                query: ::std::sync::Arc<$query>,
            ) -> Self {
                Self::from_engines(
                    ::std::sync::Arc::new($crate::domain::DisabledCommandEngine::new(ctx)),
                    query,
                )
            }
        }
    };

    (@from_query $domain:ident, $query:ty, $service:ty) => {
        impl<S: $crate::datastore::DataStore + 'static> $domain<S> {
            /// Build a query-only domain with a disabled command slot.
            pub fn from_query(
                ctx: $crate::base::EngineContext,
                query: ::std::sync::Arc<$query>,
                service: ::std::sync::Arc<$service>,
            ) -> Self {
                Self::from_engines(
                    ::std::sync::Arc::new($crate::domain::DisabledCommandEngine::new(ctx)),
                    query,
                    service,
                )
            }
        }
    };

    // ---- Struct + Domain impl + inherent helpers ---------------------------------------------
    (@core $domain:ident, $command:ident, $query:ty,
        namespace = $namespace:expr, title = $title:expr, description = $description:expr,
        $( capabilities { command: $command_capability:tt, query: $query_capability:tt } )?
        $( service = $service:ty , )?
        $( routes ( $routes_param:ident ) { $( $routes_body:tt )* } )?
    ) => {
        pub struct $domain<S: $crate::datastore::DataStore + 'static> {
            command: ::std::sync::Arc<$command<S>>,
            query: ::std::sync::Arc<$query>,
            $( service: ::std::sync::Arc<$service>, )?
        }

        impl<S: $crate::datastore::DataStore + 'static> ::core::clone::Clone for $domain<S> {
            fn clone(&self) -> Self {
                Self {
                    command: self.command.clone(),
                    query: self.query.clone(),
                    $( service: ::core::convert::identity::<&::std::sync::Arc<$service>>(&self.service).clone(), )?
                }
            }
        }

        impl<S: $crate::datastore::DataStore + 'static> $domain<S> {
            pub const NAMESPACE: &'static str = $namespace;
            pub const TITLE: &'static str = $title;
            pub const DESCRIPTION: ::core::option::Option<&'static str> = $description;

            pub fn engine_context(
                runtime: &$crate::domain::DomainRuntime,
            ) -> $crate::base::EngineContext {
                let mut ctx = $crate::base::EngineContext::for_domain(Self::NAMESPACE)
                    .with_title(Self::TITLE)
                    .with_deny_unknown_fields(runtime.deny_unknown_fields);
                if let Some(tenant) = runtime.tenant_id {
                    ctx = ctx.with_tenant_id(tenant);
                }
                ctx.tenant_policies = Some(runtime.tenant_policies.clone());
                ctx
            }

            pub fn from_engines(
                command: ::std::sync::Arc<$command<S>>,
                query: ::std::sync::Arc<$query>,
                $( service: ::std::sync::Arc<$service>, )?
            ) -> Self {
                Self {
                    command,
                    query,
                    $( service: ::core::convert::identity::<::std::sync::Arc<$service>>(service), )?
                }
            }

            pub fn into_arc(self) -> ::std::sync::Arc<Self> {
                $crate::domain!(@register_routes $( $routes_param, $($routes_body)* )?);
                ::std::sync::Arc::new(self)
            }

            pub fn command(&self) -> &$command<S> {
                &self.command
            }

            pub fn query(&self) -> &$query {
                &self.query
            }

            $(
                pub fn service(&self) -> &$service {
                    let svc: &::std::sync::Arc<$service> = &self.service;
                    svc
                }
            )?

            pub async fn execute_command(
                &self,
                cmdkey: &str,
                payload: ::serde_json::Value,
                target: $crate::command::CommandTarget,
            ) -> $crate::base::RiverbaseResult<::serde_json::Value> {
                $crate::domain::DomainCommandEngine::execute(
                    self.command.as_ref(),
                    $crate::domain::DomainCommandEngine::context(self.command.as_ref()),
                    cmdkey,
                    payload,
                    target,
                )
                .await
            }

            pub async fn execute_query(
                &self,
                ctx: &$crate::base::EngineContext,
                resource: &str,
                access: $crate::query::resource::QueryAccess,
                request: $crate::query::QueryRequest,
                item_id: ::core::option::Option<&str>,
            ) -> $crate::base::RiverbaseResult<::serde_json::Value> {
                $crate::domain::DomainQueryEngine::execute(
                    self.query.as_ref(),
                    ctx,
                    resource,
                    access,
                    request,
                    item_id,
                )
                .await
            }
        }

        #[::async_trait::async_trait]
        impl<S: $crate::datastore::DataStore + 'static> $crate::domain::Domain for $domain<S> {
            fn namespace(&self) -> &str {
                Self::NAMESPACE
            }

            fn title(&self) -> &str {
                Self::TITLE
            }

            fn description(&self) -> ::core::option::Option<&str> {
                Self::DESCRIPTION
            }

            fn command_dyn(&self) -> ::std::sync::Arc<dyn $crate::domain::DomainCommandEngine> {
                self.command.clone() as ::std::sync::Arc<dyn $crate::domain::DomainCommandEngine>
            }

            fn query_dyn(&self) -> ::std::sync::Arc<dyn $crate::domain::DomainQueryEngine> {
                self.query.clone() as ::std::sync::Arc<dyn $crate::domain::DomainQueryEngine>
            }

            $crate::domain!(@capability_methods $( $command_capability, $query_capability )?);

            $(
                fn service_dyn(
                    &self,
                ) -> ::core::option::Option<::std::sync::Arc<dyn $crate::domain::DomainServiceEngine>> {
                    let svc: ::std::sync::Arc<$service> = self.service.clone();
                    ::core::option::Option::Some(
                        svc as ::std::sync::Arc<dyn $crate::domain::DomainServiceEngine>,
                    )
                }
            )?
        }
    };

    (@register_routes) => {};
    (@register_routes $routes_param:ident, $($routes_body:tt)*) => {
        $crate::__riverbase_register_routes!(Self::NAMESPACE, $routes_param, { $($routes_body)* });
    };

    (@capability_methods) => {};
    (@capability_methods true, true) => {};
    (@capability_methods false, true) => {
        fn command_capability(
            &self,
        ) -> ::core::option::Option<::std::sync::Arc<dyn $crate::domain::DomainCommandEngine>> {
            ::core::option::Option::None
        }
    };
    (@capability_methods true, false) => {
        fn query_capability(
            &self,
        ) -> ::core::option::Option<::std::sync::Arc<dyn $crate::domain::DomainQueryEngine>> {
            ::core::option::Option::None
        }
    };
    (@capability_methods false, false) => {
        fn command_capability(
            &self,
        ) -> ::core::option::Option<::std::sync::Arc<dyn $crate::domain::DomainCommandEngine>> {
            ::core::option::Option::None
        }

        fn query_capability(
            &self,
        ) -> ::core::option::Option<::std::sync::Arc<dyn $crate::domain::DomainQueryEngine>> {
            ::core::option::Option::None
        }
    };
}

/// `domain!` `routes` expansion. Enabled only when `riverbase_core` is built with `http`.
#[cfg(feature = "http")]
#[macro_export]
#[doc(hidden)]
macro_rules! __riverbase_register_routes {
    ($namespace:expr, $routes_param:ident, { $($routes_body:tt)* }) => {
        riverbase_http::register_domain_http_routes($namespace, |$routes_param| {
            ::core::option::Option::Some({ $($routes_body)* })
        });
    };
}

/// `domain!` `routes` without the `http` feature — name the missing feature.
#[cfg(not(feature = "http"))]
#[macro_export]
#[doc(hidden)]
macro_rules! __riverbase_register_routes {
    ($namespace:expr, $routes_param:ident, { $($routes_body:tt)* }) => {
        compile_error!(
            "domain! `routes` requires riverbase_core feature `http` \
             (enable `riverbase_core` with `features = [\"http\"]` and depend on `riverbase_http`)"
        );
    };
}
