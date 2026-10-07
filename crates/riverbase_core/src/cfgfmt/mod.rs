//! Shared config loading: HCL and YAML → JSON, HCL-encoded JSON Schema validation.
//!
//! [`hcl-rs`](https://docs.rs/hcl-rs/latest/hcl/) parses HCL into `serde_json::Value`
//! using singular block keys (`stage`, `step`, `datapipe`, event-keyed `on`) that
//! match labeled HCL blocks. Legacy YAML list shapes are converted before validation.
//!
//! Domain-specific schemas and validators live in [`river_lilypad`] and [`river_lotus`].

pub mod convert;
mod hcl;
mod schema;

pub use hcl::{parse_hcl_process_documents, parse_hcl_to_json};
pub use schema::{compile_validator, load_json_schema_from_hcl, validate_instance};

use std::path::Path;

use serde_json::Value;

use crate::base::RiverbaseResult;

/// Config file format for loaders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    /// Yaml.
    Yaml,
    /// Hcl.
    Hcl,
}

impl ConfigFormat {
    /// Infer format from a file path extension (`.hcl` vs `.yaml`/`.yml`).
    pub fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|s| s.to_str()) {
            Some("hcl") => Self::Hcl,
            _ => Self::Yaml,
        }
    }
}

/// Parse config text into a canonical JSON value (YAML or HCL).
pub fn parse_to_json(text: &str, format: ConfigFormat) -> RiverbaseResult<Value> {
    match format {
        ConfigFormat::Yaml => convert::parse_yaml_to_json(text),
        ConfigFormat::Hcl => parse_hcl_to_json(text),
    }
}

/// Load config text from a path (format from extension).
pub fn read_config_text(path: &Path) -> RiverbaseResult<(String, ConfigFormat)> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| crate::errors::CFG_001.with_data(format!("{}: {e}", path.display())))?;
    Ok((text, ConfigFormat::from_path(path)))
}
