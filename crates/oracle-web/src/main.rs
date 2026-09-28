//! The `oracle-web` binary: the configuration from the environment ([`oracle_web::Config`]),
//! logs to stdout filtered by `RUST_LOG` (`info` when unset), the open-file limit raised, and the
//! service until SIGTERM. `oracle-web stats [--days N]` prints the service's counters instead
//! ([`print_stats`]).

use std::ffi::OsString;
use std::io::{IsTerminal as _, Write as _};
use std::process::ExitCode;

use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

/// The days `stats` reads unless `--days` says otherwise, and the most it may say: the counters
/// are kept 120 days.
const STATS_DAYS: u32 = 60;
const STATS_DAYS_AT_MOST: u32 = 120;

#[tokio::main]
async fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let stats = args.next().is_some_and(|arg| arg == "stats");
    let log = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false);
    if stats {
        // stdout carries the JSON alone.
        log.with_ansi(std::io::stderr().is_terminal())
            .with_writer(std::io::stderr)
            .init();
        return print_stats(args).await;
    }
    log.with_ansi(std::io::stdout().is_terminal()).init();
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

/// `oracle-web stats [--days N]`: every counter of today in Moscow and of the N - 1 days before
/// it, N from 1 to [`STATS_DAYS_AT_MOST`] ([`STATS_DAYS`] unless given), read from the Redis at
/// `REDIS_URL` and printed to stdout as one JSON object ([`oracle_web::Readout`]); the log goes
/// to stderr. It starts no server and reads no other variable. A bad argument or no `REDIS_URL`
/// exits with 2, a Redis that doesn't answer with 1.
async fn print_stats(mut args: impl Iterator<Item = OsString>) -> ExitCode {
    let mut days = STATS_DAYS;
    while let Some(arg) = args.next() {
        if arg != "--days" {
            eprintln!(
                "oracle-web stats: unknown argument {}; usage: oracle-web stats [--days N]",
                arg.to_string_lossy()
            );
            return ExitCode::from(2);
        }
        let Some(given) = args
            .next()
            .and_then(|given| given.to_str()?.parse::<u32>().ok())
            .filter(|given| (1..=STATS_DAYS_AT_MOST).contains(given))
        else {
            eprintln!(
                "oracle-web stats: --days takes a whole number of days from 1 to {STATS_DAYS_AT_MOST}"
            );
            return ExitCode::from(2);
        };
        days = given;
    }
    // Blank counts as unset, as for the service.
    let Some(url) = std::env::var("REDIS_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
    else {
        eprintln!("oracle-web stats: REDIS_URL isn't set; the counters are in the service's Redis");
        return ExitCode::from(2);
    };
    let store = match oracle_web::Store::redis(&url) {
        Ok(store) => store,
        Err(problem) => {
            eprintln!("oracle-web stats: REDIS_URL isn't a Redis address: {problem}");
            return ExitCode::from(2);
        }
    };
    let Some(readout) = oracle_web::read_stats(&store, days).await else {
        eprintln!("oracle-web stats: Redis didn't answer");
        return ExitCode::FAILURE;
    };
    let mut out = std::io::stdout().lock();
    let printed = serde_json::to_writer(&mut out, &readout)
        .map_err(std::io::Error::from)
        .and_then(|()| writeln!(out));
    match printed {
        Ok(()) => ExitCode::SUCCESS,
        Err(problem) => {
            eprintln!("oracle-web stats: {problem}");
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
