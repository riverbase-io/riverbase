//! Application logging setup (structured tracing).

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::config::{RiverbaseConfig, LogFormat};

/// Init logging.
pub fn init_logging(level: &str, format: LogFormat) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));
    let registry = tracing_subscriber::registry().with(filter);

    match format {
        LogFormat::Json => registry
            .with(tracing_subscriber::fmt::layer().json())
            .init(),
        LogFormat::Compact => registry
            .with(tracing_subscriber::fmt::layer().compact().with_target(true))
            .init(),
        LogFormat::Pretty => registry
            .with(tracing_subscriber::fmt::layer().pretty())
            .init(),
    }
}

/// Init logging from config.
pub fn init_logging_from_config(config: &RiverbaseConfig) {
    init_logging(&config.log_level, config.log_format);
}
