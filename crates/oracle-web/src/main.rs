//! The `oracle-web` binary: the configuration from the environment ([`oracle_web::Config`]),
//! logs to stdout filtered by `RUST_LOG` (`info` when unset), the open-file limit raised, and the
//! service until SIGTERM.

use std::io::IsTerminal as _;
use std::process::ExitCode;

use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};
use tracing::{error, info, warn};
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
    raise_open_files();
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

/// Raises the soft limit on open files to the hard one, as Go programs do by themselves: every
/// event stream holds a socket, and a container can start with a soft limit of 1024 under a hard
/// one of 524288, as the dev lane's pods do.
fn raise_open_files() {
    let limit = getrlimit(Resource::Nofile);
    if limit.current == limit.maximum {
        return;
    }
    let show = |files: Option<u64>| {
        files.map_or_else(|| "unlimited".to_owned(), |files| files.to_string())
    };
    let raised = Rlimit {
        current: limit.maximum,
        maximum: limit.maximum,
    };
    match setrlimit(Resource::Nofile, raised) {
        Ok(()) => info!(
            from = %show(limit.current),
            to = %show(limit.maximum),
            "raised the open-file limit"
        ),
        Err(error) => {
            warn!(%error, limit = %show(limit.current), "couldn't raise the open-file limit")
        }
    }
}
