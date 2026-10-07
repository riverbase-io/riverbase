//! Shared concurrency-pool sizing for command and query engines.
//!
//! Preferred meta keys are `command_pool_size` (command engines) and `query_pool_size`
//! (query engines). The historical `actor_pool_size` key remains accepted as an alias.
//! See [`crate` docs glossary](../../../../docs/02-design/10-glossary.md).

/// Default concurrent command/query lanes per domain engine (when meta omits pool size).
pub const DEFAULT_ACTOR_POOL_SIZE: u32 = 4;
/// Alias of [`DEFAULT_ACTOR_POOL_SIZE`] for command engines.
pub const DEFAULT_COMMAND_POOL_SIZE: u32 = DEFAULT_ACTOR_POOL_SIZE;
/// Alias of [`DEFAULT_ACTOR_POOL_SIZE`] for query engines.
pub const DEFAULT_QUERY_POOL_SIZE: u32 = DEFAULT_ACTOR_POOL_SIZE;

/// Upper bound for engine meta pool sizes.
pub const MAX_ACTOR_POOL_SIZE: u32 = 32;
/// Alias of [`MAX_ACTOR_POOL_SIZE`].
pub const MAX_COMMAND_POOL_SIZE: u32 = MAX_ACTOR_POOL_SIZE;
/// Alias of [`MAX_ACTOR_POOL_SIZE`].
pub const MAX_QUERY_POOL_SIZE: u32 = MAX_ACTOR_POOL_SIZE;

/// Clamp engine meta pool size to `[1, MAX_ACTOR_POOL_SIZE]`.
pub const fn clamp_actor_pool_size(size: u32) -> u32 {
    if size < 1 {
        1
    } else if size > MAX_ACTOR_POOL_SIZE {
        MAX_ACTOR_POOL_SIZE
    } else {
        size
    }
}

/// Alias of [`clamp_actor_pool_size`] for command engines.
pub const fn clamp_command_pool_size(size: u32) -> u32 {
    clamp_actor_pool_size(size)
}

/// Alias of [`clamp_actor_pool_size`] for query engines.
pub const fn clamp_query_pool_size(size: u32) -> u32 {
    clamp_actor_pool_size(size)
}
