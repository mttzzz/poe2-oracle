//! The `oracle-web` binary: the configuration from the environment ([`oracle_web::Config`]),
//! logs to stdout filtered by `RUST_LOG` (`info` when unset), and the service until SIGTERM.

use std::io::IsTerminal as _;
use std::process::ExitCode;

use tracing::error;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .with_ansi(std::io::stdout().is_terminal())
        .init();
    let config = match oracle_web::Config::from_env() {
        Ok(config) => config,
        Err(problem) => {
            error!(%problem, "bad configuration");
            return ExitCode::FAILURE;
        }
    };
    match oracle_web::run(config).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(problem) => {
            error!(%problem, "the service stopped");
            ExitCode::FAILURE
        }
    }
}
