//! Casbin authorization for flrs **domain activities** (audit `activity_type` values).
//!
//! Pair with [`crate::auth::Principal`] at the HTTP boundary, then call
//! [`ActivityAuthorizer::enforce`] before executing commands or emitting activities.
//!
//! Domain-specific policies and command→activity mappings belong in example/domain
//! crates (see `todo_domain::casbin` when the `http` feature is enabled).

mod activity;
mod authorizer;
#[cfg(feature = "auth")]
mod command_gate;
mod model;
mod pg_adapter;
mod policy_filter;
mod policy_pack;
mod setup;

pub use activity::{ActivityRequest, DomainActivity};
pub use authorizer::{ActivityAuthorizer, CasbinActivityAuthorizer};
#[cfg(feature = "auth")]
pub use command_gate::CasbinCommandGate;
pub use model::default_activity_model;
pub use pg_adapter::PgCasbinAdapter;
pub use policy_filter::{CasbinPolicyFilterProvider, PolicyFilterRule};
pub use policy_pack::{
    assert_commands_have_policy_rows, detect_policy_drift, missing_command_policy_rows,
    normalize_policy_csv, policy_covers_command,
};
pub use setup::build_authorizer;

pub use crate::base::{RiverbaseError, RiverbaseResult};

#[cfg(feature = "auth")]
pub use activity::ActivityRequestExt;
