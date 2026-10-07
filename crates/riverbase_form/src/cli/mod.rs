//! Command-line interface for riverbase_form.

pub mod codegen;
pub mod validate;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "riverbase_form",
    about = "Declarative form and document template tools"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Validate element/form/document HCL configs under a base directory.
    Validate {
        /// Base directory containing `elements/`, `forms/`, and/or `documents/` subdirs.
        #[arg(value_name = "DIR")]
        dir: PathBuf,
    },
    /// Generate Diesel schema, entities, and SQL migrations from element HCL files.
    Codegen {
        /// Directory containing element `*.hcl` files.
        #[arg(long, value_name = "DIR")]
        elements: PathBuf,
        /// Output directory for generated Rust (`mod.rs`, `schema.rs`, `entities.rs`).
        #[arg(long, value_name = "DIR")]
        out_rs: PathBuf,
        /// Output directory for SQL migrations.
        #[arg(long, value_name = "DIR")]
        out_migrations: PathBuf,
    },
}

impl Cli {
    pub async fn dispatch(self) -> Result<(), riverbase_core::base::RiverbaseError> {
        match self.command {
            Command::Validate { dir } => validate::validate_dir(&dir),
            Command::Codegen {
                elements,
                out_rs,
                out_migrations,
            } => codegen::codegen_elements(&elements, &out_rs, &out_migrations),
        }
    }
}
