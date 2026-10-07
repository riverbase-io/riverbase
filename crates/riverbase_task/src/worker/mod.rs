//! Worker entrypoints for message/event consumption.

pub mod consumer;
pub mod runtime;

pub use consumer::MessageConsumer;
#[cfg(feature = "nats-io")]
pub use consumer::NatsMessageConsumer;
pub use runtime::WorkerRuntime;
