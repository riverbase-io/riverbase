//! `riverbase_form` CLI entry point.

use clap::Parser;
use riverbase_form::cli::Cli;

#[tokio::main]
async fn main() {
    riverbase_core::init_logging("info", riverbase_core::LogFormat::Compact);

    let cli = Cli::parse();
    if let Err(e) = cli.dispatch().await {
        eprintln!("error [{}]: {}", e.errcode, e.errmesg);
        std::process::exit(1);
    }
}
