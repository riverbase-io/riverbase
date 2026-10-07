//! RxDB replication endpoints.
//!
//! Mirrors the Python `riverbase.rxdb` package: a [`RxdbCollection`] is the
//! abstraction for an RxDB replication endpoint (pull/push + scope
//! authorization), and [`RxdbRegistry`] mounts the corresponding HTTP routes —
//! the Rust counterpart of `RTCBridge.registerRxdbCollection`.
//!
//! ```ignore
//! use std::sync::Arc;
//! use riverbase_core::rxdb::{RxdbRegistry, RxdbCollection};
//!
//! let rxdb = RxdbRegistry::new()
//!     .with_message_bus(bus)
//!     .register(Arc::new(my_collection))?
//!     .into_router();
//! let app = coupled_router.merge(rxdb);
//! ```

pub mod collection;
pub mod router;
pub mod service;
pub mod stream;

pub use collection::{
    RxdbCollection, RxdbContext, RxdbPullRequest, RxdbPullResult, RxdbPushRequest, RxdbPushResult,
    DEFAULT_PULL_LIMIT, DEFAULT_RESOURCE_NAME, MAX_PULL_LIMIT,
};
pub use router::{RxdbRegistry, DEFAULT_RXDB_PREFIX};
pub use service::{
    handle_pull, handle_push, normalize_limit, normalize_push_rows, publish_notification,
    rxdb_notify_channel, RXDB_NOTIFY_PREFIX, RXDB_NOTIFY_TYPE,
};
pub use stream::stream_event_data;
