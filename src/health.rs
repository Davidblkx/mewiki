//! The health check: the `/_/health` route, and the `mewiki health` probe the Docker `HEALTHCHECK` runs.

use std::fs;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::App;
use crate::store::DataDir;

/// `GET /_/health`: `200` when the data folder can be read and written, otherwise `503`.
pub async fn handler(State(app): State<Arc<App>>) -> Response {
    match tokio::task::spawn_blocking(move || check_data(&app.data)).await {
        Ok(Ok(())) => (StatusCode::OK, "ok").into_response(),
        Ok(Err(e)) => {
            tracing::warn!("health check failed: {e}");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "the data folder can't be read or written",
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("health check didn't finish: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

fn check_data(data: &DataDir) -> io::Result<()> {
    fs::read_dir(data.pages())?;
    let probe = data.cache().join(".health.tmp");
    fs::write(&probe, b"ok")?;
    fs::remove_file(&probe)
}

/// Sends `GET /_/health` to the server listening on `addr` and returns `Ok` when it answers `200`.
///
/// An unspecified address such as `0.0.0.0` is probed on loopback. Uses a plain TCP socket so the image needs no
/// HTTP client.
pub fn probe(addr: SocketAddr) -> Result<(), String> {
    let target = match addr.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => SocketAddr::new(Ipv4Addr::LOCALHOST.into(), addr.port()),
        IpAddr::V6(ip) if ip.is_unspecified() => SocketAddr::new(Ipv6Addr::LOCALHOST.into(), addr.port()),
        _ => addr,
    };
    let timeout = Duration::from_secs(3);
    let mut stream = TcpStream::connect_timeout(&target, timeout).map_err(|e| format!("can't reach {target}: {e}"))?;
    stream.set_read_timeout(Some(timeout)).map_err(|e| e.to_string())?;
    stream
        .write_all(b"GET /_/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .map_err(|e| e.to_string())?;
    let mut answer = String::new();
    stream.read_to_string(&mut answer).map_err(|e| e.to_string())?;
    let status_line = answer.lines().next().unwrap_or_default();
    if status_line.split_whitespace().nth(1) == Some("200") {
        Ok(())
    } else {
        Err(format!("unhealthy: {status_line}"))
    }
}
