use std::process::ExitCode;

use mewiki::config::Config;
use mewiki::store::DataDir;
use mewiki::{App, health, routes};
use tracing_subscriber::EnvFilter;

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    if std::env::args().nth(1).as_deref() == Some("health") {
        return health_command();
    }

    let config = match Config::from_env() {
        Ok(config) => config,
        Err(message) => {
            tracing::error!("{message}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(message) = drop_root(config.uid, config.gid) {
        tracing::error!("{message}");
        return ExitCode::FAILURE;
    }
    match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime.block_on(serve(config)),
        Err(e) => {
            tracing::error!("can't start the async runtime: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `mewiki health`: exits `0` when the server on `MEWIKI_ADDR` answers its health check.
fn health_command() -> ExitCode {
    let result = Config::listen_addr().and_then(health::probe);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

async fn serve(config: Config) -> ExitCode {
    let app = match App::new(DataDir::new(&config.data_dir), &config.password, config.cookie_secure) {
        Ok(app) => app,
        Err(e) => {
            tracing::error!("can't open the data folder {}: {e}", config.data_dir.display());
            return ExitCode::FAILURE;
        }
    };
    let listener = match tokio::net::TcpListener::bind(config.addr).await {
        Ok(listener) => listener,
        Err(e) => {
            tracing::error!("can't listen on {}: {e}", config.addr);
            return ExitCode::FAILURE;
        }
    };

    tracing::info!("serving {} on http://{}", config.data_dir.display(), config.addr);
    match axum::serve(listener, routes::router(app))
        .with_graceful_shutdown(shutdown_signal())
        .await
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("server stopped: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Resolves on Ctrl+C, or on `SIGTERM`, which `docker stop` sends.
async fn shutdown_signal() {
    let interrupt = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = interrupt => {}
        () = terminate => {}
    }
}

/// Switches from root to `uid` and `gid` before anything touches the data folder or the network, so a compromised
/// server isn't root. Does nothing when the process doesn't start as root, for example under `docker run --user`.
#[cfg(target_os = "linux")]
fn drop_root(uid: u32, gid: u32) -> Result<(), String> {
    use nix::unistd::{Gid, Uid, geteuid, setgid, setgroups, setuid};
    if !geteuid().is_root() {
        return Ok(());
    }
    let (uid, gid) = (Uid::from_raw(uid), Gid::from_raw(gid));
    setgroups(&[gid]).map_err(|e| format!("can't set the supplementary groups to {gid}: {e}"))?;
    setgid(gid).map_err(|e| format!("can't switch to group {gid}: {e}"))?;
    setuid(uid).map_err(|e| format!("can't switch to user {uid}: {e}"))?;
    tracing::info!("running as user {uid}, group {gid}");
    Ok(())
}

/// Outside Linux the server runs as whoever started it.
#[cfg(not(target_os = "linux"))]
fn drop_root(_uid: u32, _gid: u32) -> Result<(), String> {
    Ok(())
}
