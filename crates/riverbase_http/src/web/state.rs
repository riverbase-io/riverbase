use std::sync::Arc;

use crate::api_path::{normalize_api_base, DEFAULT_API_BASE};
use crate::config::{HookTokenConfig, LinkTokenConfig};
use crate::domain::{Domain, DomainCommandEngine, DomainQueryEngine};

#[derive(Clone)]
/// Command app state structure.
pub struct CommandAppState {
    /// Command.
    pub command: Arc<dyn DomainCommandEngine>,
    /// Link token.
    pub link_token: LinkTokenConfig,
    /// Hook token.
    pub hook_token: HookTokenConfig,
    /// Api base.
    pub api_base: String,
    /// Api zone.
    pub api_zone: Arc<Vec<String>>,
}

impl CommandAppState {
    /// Construct a new value.
    pub fn new(command: Arc<dyn DomainCommandEngine>) -> Self {
        Self::with_api_base(command, DEFAULT_API_BASE)
    }

    /// Set api base and return self.
    pub fn with_api_base(
        command: Arc<dyn DomainCommandEngine>,
        api_base: impl Into<String>,
    ) -> Self {
        Self {
            command,
            link_token: LinkTokenConfig::default(),
            hook_token: HookTokenConfig::default(),
            api_base: normalize_api_base(&api_base.into()),
            api_zone: Arc::new(Vec::new()),
        }
    }

    /// Set link token and return self.
    pub fn with_link_token(mut self, link_token: LinkTokenConfig) -> Self {
        self.link_token = link_token;
        self
    }

    /// Set hook token and return self.
    pub fn with_hook_token(mut self, hook_token: HookTokenConfig) -> Self {
        self.hook_token = hook_token;
        self
    }
}

#[derive(Clone)]
/// Query app state structure.
pub struct QueryAppState {
    /// Query.
    pub query: Arc<dyn DomainQueryEngine>,
    /// Api base.
    pub api_base: String,
    /// Api zone.
    pub api_zone: Arc<Vec<String>>,
}

impl QueryAppState {
    /// Construct a new value.
    pub fn new(query: Arc<dyn DomainQueryEngine>) -> Self {
        Self::with_api_base(query, DEFAULT_API_BASE)
    }

    /// Set api base and return self.
    pub fn with_api_base(query: Arc<dyn DomainQueryEngine>, api_base: impl Into<String>) -> Self {
        Self {
            query,
            api_base: normalize_api_base(&api_base.into()),
            api_zone: Arc::new(Vec::new()),
        }
    }
}

#[derive(Clone)]
/// Coupled app state structure.
pub struct CoupledAppState {
    /// Command.
    pub command: CommandAppState,
    /// Query.
    pub query: QueryAppState,
}

impl CoupledAppState {
    /// Build from domain.
    pub fn from_domain<D: Domain + ?Sized>(domain: &D) -> Self {
        Self::from_domain_with_api_base(domain, DEFAULT_API_BASE)
    }

    /// Build from domain with api base.
    pub fn from_domain_with_api_base<D: Domain + ?Sized>(
        domain: &D,
        api_base: impl Into<String>,
    ) -> Self {
        let api_base = normalize_api_base(&api_base.into());
        Self {
            command: CommandAppState::with_api_base(domain.command_dyn(), &api_base),
            query: QueryAppState::with_api_base(domain.query_dyn(), &api_base),
        }
    }
}
