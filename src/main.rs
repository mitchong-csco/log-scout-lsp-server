//! Log Scout Analyzer - LSP Server
//!
//! Language Server Protocol implementation for log file analysis.
//! Provides pattern matching, diagnostics, and timeline analysis for log files.

use anyhow::Result;
use log_scout_lsp_server::LogScoutServer;
use std::fs::{self, OpenOptions};
use std::path::PathBuf;
use tower_lsp::{LspService, Server};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() -> Result<()> {
    // Get log file path
    let log_path = get_log_file_path();

    // Initialize file logging
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .expect("Failed to open log file");

    // Initialize logging (writes to both stderr and file)
    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(
            fmt::layer()
                .with_writer(move || log_file.try_clone().expect("Failed to clone log file"))
                .with_ansi(false),
        )
        .with(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    tracing::info!(
        "Starting Log Scout LSP Server v{}",
        env!("CARGO_PKG_VERSION")
    );
    tracing::info!("Log file: {}", log_path.display());

    // Run server in stdio mode (communicates via stdin/stdout)
    run_stdio_mode().await?;

    Ok(())
}

/// Get the log file path in user's home directory or temp directory
fn get_log_file_path() -> PathBuf {
    let log_dir = if let Some(home) = dirs::home_dir() {
        home.join(".log-scout-analyzer")
    } else {
        std::env::temp_dir().join("log-scout-analyzer")
    };

    // Create directory if it doesn't exist
    fs::create_dir_all(&log_dir).ok();

    // Use date-based log file name
    let date = chrono::Local::now().format("%Y-%m-%d");
    log_dir.join(format!("lsp-server-{}.log", date))
}

/// Run server in stdio mode (default for embedded deployment)
async fn run_stdio_mode() -> Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| LogScoutServer::new(client));

    tracing::info!("LSP Server running in stdio mode");
    Server::new(stdin, stdout, socket).serve(service).await;

    Ok(())
}
