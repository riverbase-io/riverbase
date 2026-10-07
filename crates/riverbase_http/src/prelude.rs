//! Common imports for a portal / service crate.
//!
//! ```ignore
//! use riverbase_http::prelude::*;
//! ```

#![warn(missing_docs)]

pub use crate::web::{RiverbaseApp, Posture, RouteAccess};
pub use crate::{
    domain_http_routes, register_domain_http_routes, ApplicationModule, PortalComposer, PortalSpec,
};

pub use riverbase_core::prelude::{
    Domain, DomainRuntime, EngineContext, RiverbaseError, RiverbaseResult,
};
