use std::process::ExitCode;

use mewiki::config::Config;
use mewiki::store::DataDir;
use mewiki::{App, routes};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let config = match Config::from_env() {
        Ok(config) => config,
        Err(message) => {
            tracing::error!("{message}");
            return ExitCode::FAILURE;
        }
    };
    let app = match App::new(DataDir::new(&config.data_dir)) {
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
    let server = axum::serve(listener, routes::router(app)).with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    });
    match server.await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("server stopped: {e}");
            ExitCode::FAILURE
        }
    }
}
