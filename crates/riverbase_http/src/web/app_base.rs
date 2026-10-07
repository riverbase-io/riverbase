//! Unified HTTP application bootstrap — mirrors Python `riverbase.fastapi.create_app`.
//!
//! [`RiverbaseApp`] loads configuration, initializes logging, accumulates OpenAPI-backed
//! routes, and serves the final axum application with standard CORS and tracing layers.

use std::sync::Arc;

use aide::axum::ApiRouter;
use aide::openapi::OpenApi;
use axum::extract::DefaultBodyLimit;
#[cfg(feature = "auth")]
use axum::extract::Extension;
use axum::http::{header, HeaderName, HeaderValue, Method};
use axum::Router;
use tokio::net::TcpListener;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;

use super::body_limit::enforce_request_body_limit;
use super::correlation::correlation_middleware;
use super::health::{health_routes, ReadinessState};
use super::request_log::log_http_request;
#[cfg(feature = "auth")]
use super::route_auth::{register_framework_public_routes, RouteAuthState};
use super::startup_config::{log_startup_report, validate_startup_config};

use crate::applog::init_logging_from_config;
use crate::base::{DomainTenantLookup, RiverbaseError, RiverbaseResult, TenantAccessFn};
use crate::config::{AuthProvider, RiverbaseConfig};
use crate::domain::{Domain, DomainRuntime};

use super::openapi::{default_coupled_openapi, finish_with_openapi};
use super::router::{command_router, coupled_router, query_router};
use super::state::{CommandAppState, CoupledAppState, QueryAppState};

#[cfg(feature = "auth")]
use super::auth_apply::{
    apply_auth, auth_disabled, auth_routes_enabled, configure_auth_router, flrs_auth_force_disabled,
};
#[cfg(feature = "auth")]
use super::casbin_layer::{apply_casbin, build_casbin_state};
#[cfg(feature = "auth")]
use crate::auth::{AuthProfileProvider, EmptyAuthProfileProvider, SessionLayer};

fn io_error(context: &str, e: std::io::Error) -> RiverbaseError {
    crate::errors::APP_001.with_data(format!("{context}: {e}"))
}

/// Build a CORS layer. Wildcard origin plus credentials is a named error ([CFG-04]).
pub fn cors_layer(
    origins: impl IntoIterator<Item = impl AsRef<str>>,
    allow_credentials: bool,
) -> RiverbaseResult<CorsLayer> {
    let origins: Vec<String> = origins
        .into_iter()
        .map(|origin| origin.as_ref().trim().to_string())
        .filter(|origin| !origin.is_empty())
        .collect();
    if origins.iter().any(|origin| origin == "*") && allow_credentials {
        return Err(crate::errors::CFG_150.raise());
    }
    if origins.is_empty() {
        return Err(crate::errors::CFG_205.raise());
    }
    // tower-http 0.6+ panics if credentials are combined with `Any` for headers
    // or methods (`Access-Control-Allow-*: *` is invalid CORS with credentials).
    let mut layer = if allow_credentials {
        CorsLayer::new()
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::PATCH,
                Method::DELETE,
                Method::HEAD,
                Method::OPTIONS,
            ])
            .allow_headers([
                header::ACCEPT,
                header::AUTHORIZATION,
                header::CONTENT_TYPE,
                HeaderName::from_static("idempotency-key"),
                HeaderName::from_static("x-csrf-token"),
                HeaderName::from_static("x-device-token"),
                HeaderName::from_static("x-profile"),
                HeaderName::from_static("x-request-id"),
            ])
    } else {
        CorsLayer::new()
            .allow_methods(tower_http::cors::Any)
            .allow_headers(tower_http::cors::Any)
    };
    if origins.iter().any(|origin| origin == "*") {
        layer = layer.allow_origin(AllowOrigin::any());
    } else {
        let values = origins
            .iter()
            .map(|origin| {
                HeaderValue::from_str(origin)
                    .map_err(|e| crate::errors::CFG_206.with_data(e.to_string()))
            })
            .collect::<RiverbaseResult<Vec<_>>>()?;
        layer = layer.allow_origin(values);
    }
    if allow_credentials {
        layer = layer.allow_credentials(true);
    }
    Ok(layer)
}

/// Deployment posture declared by the portal ([CFG-06]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Posture {
    /// Discovery-oriented service (OpenAPI, health, examples).
    OpenApi,
    /// Authenticated API: auth and Casbin must be on.
    AuthenticatedApi,
}

/// Authorization decision required when registering extra routes ([SEC-05]).
#[derive(Debug, Clone)]
pub enum RouteAccess {
    /// Participate in Casbin path→activity mapping; unmapped paths return 403 `CAS-011`.
    Enforced,
    /// Explicit public exemption (reason + sign-off).
    Public {
        /// Reason.
        reason: &'static str,
        /// Signoff.
        signoff: &'static str,
    },
}

/// Builder for a flrs HTTP service: config, OpenAPI, route composition, CORS, trace, serve.
pub struct RiverbaseApp {
    /// Config.
    pub config: RiverbaseConfig,
    /// Initialized domain runtime.
    pub runtime: Option<DomainRuntime>,
    policy_pack: Option<String>,
    casbin_known_roles: Vec<String>,
    api: OpenApi,
    router: ApiRouter,
    bind_addr: String,
    cors: Option<CorsLayer>,
    extra: Option<Router>,
    /// When `Some`, overrides [`RiverbaseConfig::log_http_requests`].
    log_http_requests: Option<bool>,
    readiness: ReadinessState,
    mounted_domains: usize,
    posture: Option<Posture>,
    #[cfg(feature = "auth")]
    route_auth: RouteAuthState,
    #[cfg(feature = "auth")]
    session_layer: Option<SessionLayer>,
    #[cfg(feature = "auth")]
    auth_routes_mounted: bool,
    #[cfg(feature = "auth")]
    auth_profile_provider: Option<Arc<dyn AuthProfileProvider>>,
    /// After auth, require one of these profile/IAM roles (`sys-admin` always passes).
    #[cfg(feature = "auth")]
    portal_roles: Vec<String>,
}

/// Listen address: named env override, else Riverbase config (`bind_addr` / `RIVERBASE_BIND_ADDR`), else default.
fn resolve_portal_bind_addr(
    bind_env_value: Option<String>,
    config_bind: &str,
    default_bind: &str,
) -> String {
    match bind_env_value {
        Some(value) if !value.trim().is_empty() => value,
        _ if !config_bind.trim().is_empty() => config_bind.to_string(),
        _ => default_bind.to_string(),
    }
}

impl RiverbaseApp {
    /// Load [`RiverbaseConfig`], initialize logging, and prepare an OpenAPI document.
    pub fn init(title: impl Into<String>, description: impl Into<String>) -> RiverbaseResult<Self> {
        let config = RiverbaseConfig::load()?;
        init_logging_from_config(&config);
        Ok(Self::with_config(config, title, description))
    }

    /// Portal entry: config, shared runtime, bind-env override, optional Casbin policy pack.
    pub async fn init_portal(
        title: impl Into<String>,
        description: impl Into<String>,
        bind_env: &str,
        default_bind: &str,
        policy_pack: Option<&str>,
    ) -> RiverbaseResult<Self> {
        #[cfg(feature = "cfgfetch")]
        let config = RiverbaseConfig::load_async().await?;
        #[cfg(not(feature = "cfgfetch"))]
        let config = RiverbaseConfig::load()?;
        init_logging_from_config(&config);
        let bind_addr = resolve_portal_bind_addr(
            std::env::var(bind_env).ok(),
            &config.bind_addr,
            default_bind,
        );
        let runtime = DomainRuntime::from_config(&config).await?;
        Ok(Self {
            config: config.clone(),
            runtime: Some(runtime),
            policy_pack: policy_pack.map(str::to_string),
            casbin_known_roles: Vec::new(),
            api: default_coupled_openapi(title, description),
            router: ApiRouter::new(),
            bind_addr,
            cors: None,
            extra: None,
            log_http_requests: None,
            readiness: ReadinessState::new(),
            mounted_domains: 0,
            posture: None,
            #[cfg(feature = "auth")]
            route_auth: RouteAuthState::new(),
            #[cfg(feature = "auth")]
            session_layer: None,
            #[cfg(feature = "auth")]
            auth_routes_mounted: false,
            #[cfg(feature = "auth")]
            auth_profile_provider: None,
            #[cfg(feature = "auth")]
            portal_roles: Vec::new(),
        })
    }

    /// Test harness: pre-built runtime, auth off, Casbin enabled by default.
    pub fn from_runtime(
        runtime: DomainRuntime,
        title: impl Into<String>,
        description: impl Into<String>,
        policy_pack: Option<&str>,
    ) -> Self {
        let mut config = RiverbaseConfig::default();
        config.auth.auth_provider = AuthProvider::None;
        config.casbin.enabled = true;
        config.auth.application_secret_key = Some("test-session-secret".into());
        config.hook_token.secret = Some("test-hook-secret".into());
        config.hook_token.salt = Some("test-hook-salt".into());
        config.link_token.secret = Some("test-link-secret".into());
        config.link_token.salt = Some("test-link-salt".into());
        config.normalize_paths();
        let app = Self {
            config,
            runtime: Some(runtime),
            policy_pack: policy_pack.map(str::to_string),
            casbin_known_roles: Vec::new(),
            api: default_coupled_openapi(title, description),
            router: ApiRouter::new(),
            bind_addr: "127.0.0.1:0".into(),
            cors: None,
            extra: None,
            log_http_requests: None,
            readiness: ReadinessState::new(),
            mounted_domains: 0,
            posture: None,
            #[cfg(feature = "auth")]
            route_auth: RouteAuthState::new(),
            #[cfg(feature = "auth")]
            session_layer: None,
            #[cfg(feature = "auth")]
            auth_routes_mounted: false,
            #[cfg(feature = "auth")]
            auth_profile_provider: None,
            #[cfg(feature = "auth")]
            portal_roles: Vec::new(),
        };
        app.mark_migrations_ready();
        app
    }

    /// Build from an existing config (tests or custom load paths).
    pub fn with_config(
        mut config: RiverbaseConfig,
        title: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        config.normalize_paths();
        let bind_addr = config.bind_addr.clone();
        Self {
            config,
            runtime: None,
            policy_pack: None,
            casbin_known_roles: Vec::new(),
            api: default_coupled_openapi(title, description),
            router: ApiRouter::new(),
            bind_addr,
            cors: None,
            extra: None,
            log_http_requests: None,
            readiness: ReadinessState::new(),
            mounted_domains: 0,
            posture: None,
            #[cfg(feature = "auth")]
            route_auth: RouteAuthState::new(),
            #[cfg(feature = "auth")]
            session_layer: None,
            #[cfg(feature = "auth")]
            auth_routes_mounted: false,
            #[cfg(feature = "auth")]
            auth_profile_provider: None,
            #[cfg(feature = "auth")]
            portal_roles: Vec::new(),
        }
    }

    /// Config.
    pub fn config(&self) -> &RiverbaseConfig {
        &self.config
    }

    /// Casbin policy pack name (`configs/policies/{pack}.csv`), set by [`Self::init_portal`].
    pub fn policy_pack(&self) -> Option<&str> {
        self.policy_pack.as_deref()
    }

    /// Bind addr.
    pub fn bind_addr(&self) -> &str {
        &self.bind_addr
    }

    /// Initialized domain runtime.
    pub fn runtime(&self) -> Option<&DomainRuntime> {
        self.runtime.as_ref()
    }

    /// Mark domain migrations complete so `/ready` can succeed ([DAT-02]).
    pub fn mark_migrations_ready(&self) {
        self.readiness.set_migrations_ready(true);
    }

    /// Readiness.
    pub fn readiness(&self) -> &ReadinessState {
        &self.readiness
    }

    /// Runtime mut.
    pub fn runtime_mut(&mut self) -> Option<&mut DomainRuntime> {
        self.runtime.as_mut()
    }

    /// Whether `--migrate-only` was passed on the command line.
    pub fn migrate_only() -> bool {
        std::env::args().any(|a| a == "--migrate-only")
    }

    /// Mount coupled.
    pub async fn mount_coupled(&mut self, mut state: CoupledAppState) -> RiverbaseResult<()> {
        state.command.api_base = self.config.api_base.clone();
        state.query.api_base = self.config.api_base.clone();
        let api_zone = Arc::new(self.config.api_zone.clone());
        state.command.api_zone = api_zone.clone();
        state.query.api_zone = api_zone;
        let routes = coupled_router(state).await?;
        self.router = self.router.clone().merge(routes);
        Ok(())
    }

    /// Mount command.
    pub async fn mount_command(&mut self, mut state: CommandAppState) -> RiverbaseResult<()> {
        state.api_base = self.config.api_base.clone();
        state.api_zone = Arc::new(self.config.api_zone.clone());
        let routes = command_router(state).await?;
        self.router = self.router.clone().merge(routes);
        Ok(())
    }

    /// Mount query.
    pub async fn mount_query(&mut self, mut state: QueryAppState) -> RiverbaseResult<()> {
        state.api_base = self.config.api_base.clone();
        state.api_zone = Arc::new(self.config.api_zone.clone());
        let routes = query_router(state).await?;
        self.router = self.router.clone().merge(routes);
        Ok(())
    }

    /// Mount a coupled domain (command + query routes); registers on shared invoker when runtime is set.
    pub async fn mount_domain<D: Domain + ?Sized>(&mut self, domain: Arc<D>) -> RiverbaseResult<()> {
        // Build and validate the machine-readable contract before exposing any routes.
        let _manifest = domain.discovery().await?;
        let command = domain.command_capability();
        let query = domain.query_capability();
        if let Some(ref query_engine) = query {
            super::startup_checks::validate_mounted_query_engine(
                domain.namespace(),
                query_engine.as_ref(),
            )
            .await?;
        }
        if let (Some(runtime), Some(command)) = (self.runtime.as_mut(), command.clone()) {
            runtime.invoker.register(domain.namespace(), command);
        }
        if let (Some(runtime), Some(query)) = (self.runtime.as_mut(), query.clone()) {
            runtime.query_invoker.register(domain.namespace(), query);
        }

        match (command, query) {
            (Some(command), Some(query)) => {
                let state = CoupledAppState {
                    command: CommandAppState::with_api_base(command, &self.config.api_base),
                    query: QueryAppState::with_api_base(query, &self.config.api_base),
                };
                self.mount_coupled(state).await?;
            }
            (Some(command), None) => {
                self.mount_command(CommandAppState::with_api_base(
                    command,
                    &self.config.api_base,
                ))
                .await?;
            }
            (None, Some(query)) => {
                self.mount_query(QueryAppState::with_api_base(query, &self.config.api_base))
                    .await?;
            }
            (None, None) => {}
        }

        if let Some(routes) =
            crate::routes_registry::domain_http_routes(domain.namespace(), &self.config.api_base)
        {
            self.register_routes(routes, RouteAccess::Enforced);
        }
        #[cfg(feature = "auth")]
        {
            self.route_auth
                .register_policy_identity(domain.namespace(), domain.policy_identity());
        }
        self.mounted_domains += 1;
        Ok(())
    }

    #[cfg(feature = "auth")]
    /// Mount auth routes.
    pub async fn mount_auth_routes(&mut self) -> RiverbaseResult<()> {
        use std::sync::Arc;

        use crate::auth::DefaultAuthProfileProvider;
        use crate::auth::{AuthProfileProvider, EmptyAuthProfileProvider, JwtValidator};

        let provider: Arc<dyn AuthProfileProvider> = if self.config.auth.uses_keycloak_auth() {
            let validator = Arc::new(JwtValidator::new(crate::auth::to_oidc_config(
                &self.config.auth,
            ))?);
            Arc::new(DefaultAuthProfileProvider::new(
                self.config.auth.clone(),
                validator,
            ))
        } else {
            Arc::new(EmptyAuthProfileProvider)
        };
        self.mount_auth_routes_with(provider).await
    }

    /// Mount `/api/auth/*` with a custom [`AuthProfileProvider`] for `/auth/profiles`.
    #[cfg(feature = "auth")]
    pub async fn mount_auth_routes_with(
        &mut self,
        profile_provider: std::sync::Arc<dyn crate::auth::AuthProfileProvider>,
    ) -> RiverbaseResult<()> {
        if self.auth_routes_mounted {
            return Ok(());
        }
        if !auth_routes_enabled(&self.config.auth) {
            // Explicit opt-out (`RIVERBASE_AUTH_DISABLED=1`) is allowed to skip silently;
            // a missing/`None` provider is treated as a misconfiguration so the
            // service fails fast instead of serving 404s on `/api/auth/*`.
            if flrs_auth_force_disabled() {
                return Ok(());
            }
            return Err(crate::errors::CFG_179.with_data(format!(
                "auth_provider = {:?}",
                self.config.auth.auth_provider
            )));
        }
        let pool = self.runtime.as_ref().map(|rt| match &rt.dbconn {
            crate::domain::DbConnection::Postgres(conn) => conn.pgpool.clone(),
        });
        let pool = pool.ok_or_else(|| {
            crate::errors::AUT_181.with_data("RiverbaseApp.runtime missing Postgres pool")
        })?;
        let (auth_api, session_layer) =
            configure_auth_router(&self.config.auth, profile_provider.clone(), pool).await?;
        self.router = self.router.clone().merge(auth_api);
        if session_layer.is_some() {
            self.session_layer = session_layer;
        }
        self.auth_profile_provider = Some(profile_provider);
        self.auth_routes_mounted = true;
        Ok(())
    }

    /// Mount OAuth2/OIDC browser login routes at `/auth`.
    #[cfg(feature = "auth")]
    pub async fn mount_auth(&mut self) -> RiverbaseResult<()> {
        use std::sync::Arc;

        use crate::auth::{DefaultAuthProfileProvider, JwtValidator};

        let mut auth_cfg = self.config.auth.clone();
        auth_cfg.normalize_paths(&self.config.api_base);
        let validator = Arc::new(JwtValidator::new(crate::auth::to_oidc_config(&auth_cfg))?);
        let provider = Arc::new(DefaultAuthProfileProvider::new(auth_cfg.clone(), validator));
        let pool = self.runtime.as_ref().map(|rt| match &rt.dbconn {
            crate::domain::DbConnection::Postgres(conn) => conn.pgpool.clone(),
        });
        let pool = pool.ok_or_else(|| {
            crate::errors::AUT_181.with_data("RiverbaseApp.runtime missing Postgres pool")
        })?;
        let (router, session_layer, _) =
            crate::auth::configure_authentication(auth_cfg, provider, pool).await?;
        self.session_layer = Some(session_layer);
        self.router = self.router.clone().merge(router);
        Ok(())
    }

    /// Merge extra OpenAPI-backed routes (e.g. RxDB).
    pub fn merge(&mut self, extra: ApiRouter) {
        self.router = self.router.clone().merge(extra);
    }

    /// Register the Casbin policy identity for an HTTP wire namespace ([SEC-04]).
    #[cfg(feature = "auth")]
    pub fn register_policy_identity(
        &mut self,
        wire: impl Into<String>,
        identity: impl Into<String>,
    ) {
        self.route_auth.register_policy_identity(wire, identity);
    }

    /// Declare the portal's security posture ([CFG-06]).
    pub fn require_posture(&mut self, posture: Posture) {
        self.posture = Some(posture);
    }

    /// Register extra HTTP routes that participate in Casbin activity enforcement ([SEC-05]).
    pub fn register_routes(&mut self, routes: Router, access: RouteAccess) {
        match access {
            RouteAccess::Enforced => {}
            RouteAccess::Public { reason, signoff } => {
                tracing::warn!(reason, signoff, "register_routes public exemption");
            }
        }
        self.merge_routes_inner(routes);
    }

    /// Register routes with an explicit public exemption ([SEC-06], [SEC-07]).
    pub fn register_public_routes(&mut self, routes: Router, paths: &[&str]) {
        #[cfg(feature = "auth")]
        for path in paths {
            self.route_auth.register_public_path(*path);
        }
        let _ = paths;
        self.merge_routes_inner(routes);
    }

    /// Register a public path prefix (e.g. `/v/` for ticket verify pages).
    #[cfg(feature = "auth")]
    pub fn register_public_prefix(&mut self, prefix: impl Into<String>) {
        self.route_auth.register_public_prefix(prefix);
    }

    /// Unauthenticated requests under this prefix redirect to sign-in.
    #[cfg(feature = "auth")]
    pub fn register_login_redirect_prefix(&mut self, prefix: impl Into<String>) {
        self.route_auth.register_login_redirect_prefix(prefix);
    }

    fn merge_routes_inner(&mut self, routes: Router) {
        match self.extra.take() {
            Some(existing) => self.extra = Some(existing.merge(routes)),
            None => self.extra = Some(routes),
        }
    }

    /// Deprecated: use [`Self::register_routes`] or [`Self::register_public_routes`].
    #[deprecated(note = "use register_routes or register_public_routes for explicit authorization")]
    pub fn merge_routes(&mut self, routes: Router) {
        tracing::warn!(
            "merge_routes bypasses explicit authorization registration; use register_routes or register_public_routes"
        );
        self.merge_routes_inner(routes);
    }

    /// Bind.
    pub fn bind(&mut self, addr: impl Into<String>) {
        self.bind_addr = addr.into();
    }

    /// Set cors and return self.
    pub fn with_cors(&mut self, cors: CorsLayer) {
        self.cors = Some(cors);
    }

    /// Set cors origins and return self.
    pub fn with_cors_origins(
        &mut self,
        origins: impl IntoIterator<Item = impl AsRef<str>>,
        allow_credentials: bool,
    ) -> RiverbaseResult<&mut Self> {
        self.cors = Some(cors_layer(origins, allow_credentials)?);
        Ok(self)
    }

    /// Without cors.
    pub fn without_cors(&mut self) {
        self.cors = None;
    }

    /// Set log http requests and return self.
    pub fn with_log_http_requests(&mut self, enabled: bool) {
        self.log_http_requests = Some(enabled);
    }

    /// Set casbin policy and return self.
    pub fn with_casbin_policy(&mut self, policy: impl Into<String>) {
        self.config.casbin.policy = policy.into();
    }

    /// Set casbin known roles and return self.
    pub fn with_casbin_known_roles(&mut self, roles: impl IntoIterator<Item = impl Into<String>>) {
        self.casbin_known_roles = roles.into_iter().map(|r| r.into()).collect();
    }

    /// After authentication, require one of these roles to call protected routes.
    ///
    /// `sys-admin` always passes. Profile roles and IAM realm roles both count.
    /// Public routes and unauthenticated requests are unchanged.
    #[cfg(feature = "auth")]
    pub fn require_any_role(&mut self, roles: impl IntoIterator<Item = impl Into<String>>) {
        self.portal_roles = roles.into_iter().map(|r| r.into()).collect();
    }

    fn log_http_requests_enabled(&self) -> bool {
        self.log_http_requests
            .unwrap_or(self.config.log_http_requests)
    }

    /// Register an extra named tenant access policy (in addition to builtins).
    pub fn register_tenant_access_policy(&self, name: impl Into<String>, func: TenantAccessFn) {
        if let Some(runtime) = &self.runtime {
            runtime
                .tenant_policies
                .register_tenant_access_policy(name, func);
        }
    }

    /// Install the `domain-tenant` lookup used by access and stamp policies.
    pub fn set_domain_tenant_lookup(&self, lookup: Arc<dyn DomainTenantLookup>) {
        if let Some(runtime) = &self.runtime {
            runtime.tenant_policies.set_domain_tenant_lookup(lookup);
        }
    }

    /// Finalize OpenAPI and apply HTTP middleware layers.
    pub async fn into_router(self) -> RiverbaseResult<Router> {
        validate_startup_config(&self.config)?;
        if let Some(runtime) = &self.runtime {
            runtime.tenant_policies.validate_startup()?;
        }
        if self.posture == Some(Posture::AuthenticatedApi) {
            #[cfg(feature = "auth")]
            if auth_disabled(&self.config.auth) || !self.config.casbin.enabled {
                return Err(crate::errors::CFG_152
                    .with_data("set [riverbase.auth] and [riverbase.casbin] enabled = true"));
            }
        }
        self.readiness.set_ready(false);
        let log_http = self.log_http_requests_enabled();
        #[cfg(feature = "auth")]
        register_framework_public_routes(&self.route_auth, &self.config.api_base);
        self.route_auth
            .set_public_api_prefix(crate::auth::prefix_from_callback_uri(
                &self.config.auth.default_callback_uri,
            ));
        let mut api = self.api;
        let mut router = finish_with_openapi(self.router, &mut api, &self.config.api_base);
        if let Some(extra) = self.extra {
            router = router.merge(extra);
        }
        #[cfg(feature = "auth")]
        {
            if self.config.casbin.enabled
                && self.config.casbin.omit_inaccessible_openapi
                && auth_disabled(&self.config.auth)
            {
                return Err(crate::errors::CFG_204.with_data(format!(
                    "auth_provider = {:?}",
                    self.config.auth.auth_provider
                )));
            }
            if self.config.casbin.enabled {
                let pool = self
                    .runtime
                    .as_ref()
                    .map(|rt| match &rt.dbconn {
                        crate::domain::DbConnection::Postgres(conn) => (*conn.pgpool).clone(),
                    })
                    .ok_or_else(|| {
                        crate::errors::CAS_009.with_data("RiverbaseApp.runtime missing Postgres pool")
                    })?;
                if let Some(state) = build_casbin_state(
                    pool,
                    &self.config.casbin,
                    self.policy_pack.as_deref(),
                    &self.config.api_base,
                    &self.casbin_known_roles,
                    self.route_auth.clone(),
                    !auth_disabled(&self.config.auth),
                )
                .await?
                {
                    let policy_csv = crate::web::casbin_layer::load_policy_csv(
                        &self.config.casbin,
                        self.policy_pack.as_deref(),
                    );
                    if let Some(pack) = self.policy_pack.as_deref() {
                        crate::casbin::detect_policy_drift(pack, &self.config.casbin.policy)?;
                    }
                    if let Some(runtime) = &self.runtime {
                        runtime.invoker.set_activity_gate(Arc::new(
                            crate::casbin::CasbinCommandGate::new(
                                state.authorizer.clone(),
                                state.route_auth.clone(),
                                state.known_roles.clone(),
                            ),
                        ));
                        let catalog = runtime.invoker.command_catalog().await?;
                        let missing = crate::casbin::missing_command_policy_rows(
                            &policy_csv,
                            &catalog,
                            |wire| self.route_auth.policy_identity(wire),
                        );
                        for row in &missing {
                            tracing::warn!(command = %row, "command has no Casbin policy row");
                        }
                        if !missing.is_empty() && self.posture == Some(Posture::AuthenticatedApi) {
                            return Err(crate::errors::CAS_013.with_data(missing.join(", ")));
                        }
                    }
                    if self.config.casbin.omit_inaccessible_openapi {
                        router = router.layer(Extension(state.clone()));
                    }
                    router = apply_casbin(router, state);
                }
            }
            if !self.portal_roles.is_empty() {
                router = router.layer(axum::middleware::from_fn_with_state(
                    crate::auth::middleware::PortalRoleState {
                        allowed: self.portal_roles.clone(),
                        route_auth: self.route_auth.clone(),
                    },
                    crate::auth::middleware::require_portal_roles,
                ));
            }
            router = apply_auth(
                router,
                &self.config.auth,
                self.auth_profile_provider
                    .clone()
                    .unwrap_or_else(|| Arc::new(EmptyAuthProfileProvider)),
                self.route_auth.clone(),
            )
            .await;
            if let Some(session) = self.session_layer {
                router = router.layer(session);
            }
            router = router.layer(axum::middleware::from_fn(
                crate::auth::middleware::idempotency_echo,
            ));
        }
        if let Some(cors) = self.cors {
            router = router.layer(cors);
        } else if !log_http {
            router = router.layer(TraceLayer::new_for_http());
        }
        if log_http {
            router = router.layer(axum::middleware::from_fn(log_http_request));
        }
        router = router.layer(axum::middleware::from_fn(correlation_middleware));
        router = router.layer(axum::middleware::from_fn(
            crate::http_response::fill_problem_instance,
        ));
        router = router.layer(axum::middleware::from_fn(
            crate::web::security_headers::security_headers,
        ));
        let body_limit = self.config.request_body_max_bytes;
        let upload_limit = self.config.request_upload_max_bytes;
        router = router.layer(axum::middleware::from_fn(move |req, next| {
            enforce_request_body_limit(body_limit, upload_limit, req, next)
        }));
        router = router.layer(DefaultBodyLimit::max(body_limit.max(upload_limit)));
        router = router.merge(health_routes(self.readiness.clone()));
        self.readiness.set_ready(true);
        log_startup_report(
            &self.config,
            self.mounted_domains,
            self.policy_pack.as_deref(),
            #[cfg(feature = "auth")]
            &self.route_auth,
        );
        Ok(router)
    }

    /// Sync finalize for simple services without auth/casbin (legacy).
    pub fn into_axum(self) -> Router {
        let log_http = self.log_http_requests_enabled();
        let body_limit = self.config.request_body_max_bytes;
        let upload_limit = self.config.request_upload_max_bytes;
        let mut api = self.api;
        let mut router = finish_with_openapi(self.router, &mut api, &self.config.api_base);
        if let Some(extra) = self.extra {
            router = router.merge(extra);
        }
        #[cfg(feature = "auth")]
        if let Some(session) = self.session_layer {
            router = router.layer(session);
        }
        if let Some(cors) = self.cors {
            router = router.layer(cors);
        }
        if log_http {
            router = router.layer(axum::middleware::from_fn(log_http_request));
        }
        router = router.layer(axum::middleware::from_fn(move |req, next| {
            enforce_request_body_limit(body_limit, upload_limit, req, next)
        }));
        router.layer(DefaultBodyLimit::max(body_limit.max(upload_limit)))
    }

    /// Bind and run the HTTP server until shutdown ([RUN-03]).
    pub async fn serve(self) -> RiverbaseResult<()> {
        let addr = self.bind_addr.clone();
        let readiness = self.readiness.clone();
        let runtime = self.runtime.clone();
        let router =
            if self.runtime.is_some() || self.policy_pack.is_some() || self.config.casbin.enabled {
                self.into_router().await?
            } else {
                self.into_axum()
            };
        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|e| io_error("Failed to bind listen address", e))?;
        info!(addr = %addr, "flrs service listening");
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                if tokio::signal::ctrl_c().await.is_ok() {
                    readiness.set_ready(false);
                    if let Some(rt) = runtime {
                        rt.abort_workers();
                    }
                }
            })
            .await
            .map_err(|e| io_error("HTTP server error", e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_config_uses_bind_addr() {
        let cfg = RiverbaseConfig {
            bind_addr: "127.0.0.1:9999".into(),
            ..Default::default()
        };
        let app = RiverbaseApp::with_config(cfg, "Test", "desc");
        assert_eq!(app.bind_addr, "127.0.0.1:9999");
    }

    #[test]
    fn portal_bind_prefers_env_then_riverbase_config() {
        assert_eq!(
            resolve_portal_bind_addr(Some("0.0.0.0:9000".into()), "0.0.0.0:8082", "0.0.0.0:8080"),
            "0.0.0.0:9000"
        );
        assert_eq!(
            resolve_portal_bind_addr(None, "0.0.0.0:8082", "0.0.0.0:8080"),
            "0.0.0.0:8082"
        );
        assert_eq!(
            resolve_portal_bind_addr(Some("  ".into()), "", "0.0.0.0:8080"),
            "0.0.0.0:8080"
        );
    }

    #[test]
    fn bind_overrides_addr() {
        let mut app = RiverbaseApp::with_config(RiverbaseConfig::default(), "Test", "desc");
        app.bind("0.0.0.0:3000");
        assert_eq!(app.bind_addr, "0.0.0.0:3000");
    }

    #[test]
    fn with_log_http_requests_overrides_config() {
        let cfg = RiverbaseConfig {
            log_http_requests: false,
            ..Default::default()
        };
        let app = RiverbaseApp::with_config(cfg, "Test", "desc");
        let mut app = app;
        app.with_log_http_requests(true);
        assert!(app.log_http_requests_enabled());
    }

    #[test]
    fn default_app_has_no_cors_layer() {
        let app = RiverbaseApp::with_config(RiverbaseConfig::default(), "Test", "desc");
        assert!(app.cors.is_none());
    }

    #[test]
    fn wildcard_origin_with_credentials_is_cfg_150() {
        let err = cors_layer(["*"], true).expect_err("wildcard+credentials");
        assert_eq!(err.errcode.as_str(), "CFG-150");
    }

    #[test]
    fn explicit_origins_with_credentials_builds() {
        let _ = cors_layer(["http://localhost:5181"], true).expect("cors");
    }
}
