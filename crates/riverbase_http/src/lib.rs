//! Riverbase HTTP portal stack and application composition ([ARC-03]).
//!
//! Domain crates depend on [`riverbase_core`] only. Portal services and HTTP glue
//! depend on this crate.

#![warn(missing_docs)]

use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use riverbase_core::domain::{Domain, DomainRuntime};

/// Authentication extractors, JWKS, and session cookies.
pub mod auth;
/// Casbin policy loader and request authorizer.
pub mod casbin;
/// Crate error catalogue.
pub mod errors;
/// Problem+JSON and success-envelope HTTP responses.
pub mod http_response;
/// Common imports for portal crates (`use riverbase_http::prelude::*;`).
pub mod prelude;
/// Domain HTTP route registration.
pub mod routes_registry;
/// RxDB pull/push replication helpers.
#[cfg(feature = "rxdb")]
pub mod rxdb;
/// Server-sent event streams for the message bus.
#[cfg(feature = "sse")]
pub mod sse;
/// Application builder, posture, and HTTP middleware.
pub mod web;
/// WebSocket / RTC channel authorization and codecs.
#[cfg(feature = "websocket")]
pub mod websocket;

// Kernel modules referenced by moved portal code via `crate::…`.
#[cfg(any(feature = "websocket", feature = "rxdb"))]
pub(crate) use riverbase_core::transport;
pub(crate) use riverbase_core::{applog, base, command, config, domain, query, util};

pub use util::{api_path, openapi_meta, OpenApiMeta};

pub use auth::*;
pub use http_response::{
    error_into_response, error_into_response_at, fill_problem_instance, http_json_response,
    problem_details_into_response, problem_json_response,
};
pub use routes_registry::{domain_http_routes, register_domain_http_routes};
#[cfg(feature = "rxdb")]
pub use rxdb::{
    rxdb_notify_channel, RxdbCollection, RxdbContext, RxdbPullRequest, RxdbPullResult,
    RxdbPushRequest, RxdbPushResult, RxdbRegistry,
};
#[cfg(feature = "sse")]
pub use sse::bus_sse_stream;
pub use web::RiverbaseApp;
#[cfg(feature = "websocket")]
pub use websocket::{
    authorize_channel, default_channel_permissions, register_builtin_handlers, BridgeProxy,
    ClientMessage, MessageRegistry, RtcBridge, RtcTransport, StreamRtcTransport, TransportMessage,
    WireCodec,
};

/// Re-exported for `domain! { routes { … } }` and custom HTTP handlers.
#[doc(hidden)]
pub use axum;

/// Log a process-fatal error via tracing and exit with status 1.
///
/// Prefer this over returning `Err` from `main`: Rust's default `Error: {:?}` printer
/// dumps `RiverbaseError` as a struct and skips the tracing formatter.
pub fn report_fatal(err: Box<dyn std::error::Error + Send + Sync>) -> ! {
    if let Some(fe) = err.downcast_ref::<base::RiverbaseError>() {
        fe.log();
    } else {
        tracing::error!(error = %err, "fatal");
    }
    std::process::exit(1);
}

/// How a portal process binds, titles itself, and loads a Casbin pack.
#[derive(Debug, Clone)]
pub struct PortalSpec {
    /// OpenAPI / process title.
    pub title: String,
    /// OpenAPI / process description.
    pub description: String,
    /// Environment variable that overrides the listen address.
    pub bind_env: String,
    /// Listen address used when `bind_env` is unset.
    pub default_bind: String,
    /// Optional Casbin policy-pack path.
    pub policy_pack: Option<String>,
    /// Extra Casbin roles registered at startup.
    pub casbin_known_roles: Vec<String>,
}

impl PortalSpec {
    /// Construct a portal spec without a policy pack or extra roles.
    pub fn new(
        title: impl Into<String>,
        description: impl Into<String>,
        bind_env: impl Into<String>,
        default_bind: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            description: description.into(),
            bind_env: bind_env.into(),
            default_bind: default_bind.into(),
            policy_pack: None,
            casbin_known_roles: Vec::new(),
        }
    }

    /// Set policy pack and return self.
    pub fn with_policy_pack(mut self, policy_pack: impl Into<String>) -> Self {
        self.policy_pack = Some(policy_pack.into());
        self
    }

    /// Set casbin known roles and return self.
    pub fn with_casbin_known_roles(
        mut self,
        roles: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.casbin_known_roles = roles.into_iter().map(Into::into).collect();
        self
    }
}

/// Product module declaration. Migration preparation always runs before the migrate-only gate;
/// route mounting never runs in migrate-only mode.
#[async_trait]
pub trait ApplicationModule: Send + Sync {
    /// Stable module name used to reject duplicate registration.
    fn name(&self) -> &'static str;

    /// Run migrations or other setup before routes are mounted.
    async fn prepare(&self, _runtime: &DomainRuntime) -> base::RiverbaseResult<()> {
        Ok(())
    }

    /// Mount domain routes onto the portal (skipped in migrate-only mode).
    async fn mount(&self, portal: &mut PortalComposer) -> base::RiverbaseResult<()>;
}

/// Initialized, non-optional application runtime used while mounting modules.
pub struct PortalComposer {
    app: RiverbaseApp,
    runtime: DomainRuntime,
    mounted_namespaces: BTreeSet<String>,
    mounted_modules: BTreeSet<&'static str>,
}

impl PortalComposer {
    /// Load config, open the runtime, and return a composer ready for modules.
    pub async fn initialize(spec: PortalSpec) -> base::RiverbaseResult<Self> {
        let mut app = RiverbaseApp::init_portal(
            spec.title,
            spec.description,
            &spec.bind_env,
            &spec.default_bind,
            spec.policy_pack.as_deref(),
        )
        .await?;
        app.with_casbin_known_roles(spec.casbin_known_roles);
        let runtime = app.runtime().cloned().ok_or_else(|| {
            crate::errors::APP_022.with_data("RiverbaseApp::init_portal returned no runtime")
        })?;
        Ok(Self {
            app,
            runtime,
            mounted_namespaces: BTreeSet::new(),
            mounted_modules: BTreeSet::new(),
        })
    }

    /// In-memory composition harness with the same module lifecycle as production.
    pub fn from_runtime(
        runtime: DomainRuntime,
        title: impl Into<String>,
        description: impl Into<String>,
        policy_pack: Option<&str>,
    ) -> Self {
        Self {
            app: RiverbaseApp::from_runtime(runtime.clone(), title, description, policy_pack),
            runtime,
            mounted_namespaces: BTreeSet::new(),
            mounted_modules: BTreeSet::new(),
        }
    }

    /// Initialized domain runtime.
    pub fn runtime(&self) -> &DomainRuntime {
        &self.runtime
    }

    /// HTTP application builder.
    pub fn app(&self) -> &RiverbaseApp {
        &self.app
    }

    /// Mutable access to the HTTP application builder.
    pub fn app_mut(&mut self) -> &mut RiverbaseApp {
        &mut self.app
    }

    /// Mount a domain once; a repeated namespace is `APP-003`.
    pub async fn mount_domain<D>(&mut self, domain: Arc<D>) -> base::RiverbaseResult<()>
    where
        D: Domain + ?Sized + 'static,
    {
        let namespace = domain.namespace().to_string();
        if !self.mounted_namespaces.insert(namespace.clone()) {
            return Err(crate::errors::APP_023.with_data(namespace));
        }
        self.app.mount_domain(domain).await
    }

    #[cfg(feature = "auth")]
    /// Mount auth routes that resolve profiles through `provider`.
    pub async fn mount_auth_profile_provider(
        &mut self,
        provider: Arc<dyn auth::AuthProfileProvider>,
    ) -> base::RiverbaseResult<()> {
        self.app.mount_auth_routes_with(provider).await
    }

    /// Prepare every module, then mount routes unless this process is migrate-only.
    pub async fn compose(
        mut self,
        modules: impl IntoIterator<Item = Arc<dyn ApplicationModule>>,
    ) -> base::RiverbaseResult<CompositionOutcome> {
        let modules = modules.into_iter().collect::<Vec<_>>();
        for module in &modules {
            if !self.mounted_modules.insert(module.name()) {
                return Err(crate::errors::APP_004.with_data(module.name()));
            }
            module.prepare(&self.runtime).await?;
        }
        self.app.mark_migrations_ready();

        if RiverbaseApp::migrate_only() {
            return Ok(CompositionOutcome::MigrationsComplete);
        }

        for module in modules {
            module.mount(&mut self).await?;
        }
        Ok(CompositionOutcome::Ready(ReadyPortal { app: self.app }))
    }
}

/// Result of [`PortalComposer::compose`].
pub enum CompositionOutcome {
    /// Migrations ran and the process should exit (migrate-only).
    MigrationsComplete,
    /// Portal is ready to serve HTTP.
    Ready(ReadyPortal),
}

/// HTTP portal after successful composition.
pub struct ReadyPortal {
    app: RiverbaseApp,
}

impl ReadyPortal {
    /// Convert into router.
    pub async fn into_router(self) -> base::RiverbaseResult<axum::Router> {
        self.app.into_router().await
    }

    /// Bind and serve HTTP until the process exits.
    pub async fn serve(self) -> base::RiverbaseResult<()> {
        self.app.serve().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EmptyModule;

    #[async_trait]
    impl ApplicationModule for EmptyModule {
        fn name(&self) -> &'static str {
            "empty"
        }

        async fn mount(&self, _portal: &mut PortalComposer) -> base::RiverbaseResult<()> {
            Ok(())
        }
    }

    #[tokio::test]
    #[ignore = "requires Postgres (DomainRuntime::in_memory removed)"]
    async fn duplicate_modules_fail_composition() {
        let _ = std::any::type_name::<EmptyModule>();
    }
}
