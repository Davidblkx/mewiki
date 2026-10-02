//! Settings read from the environment at startup.

use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;

/// The server's settings.
#[derive(Clone, Debug)]
pub struct Config {
    /// The data folder, from `MEWIKI_DATA`. Defaults to `/data`.
    pub data_dir: PathBuf,
    /// The listen address, from `MEWIKI_ADDR`. Defaults to `0.0.0.0:8080`.
    pub addr: SocketAddr,
    /// The only password, from `MEWIKI_PASSWORD`. Required (R12).
    pub password: String,
    /// Whether the session cookie gets the `Secure` attribute, from `MEWIKI_COOKIE_SECURE`. Only `false` turns it
    /// off.
    pub cookie_secure: bool,
    /// The user the server switches to when it starts as root, from `PUID`. Defaults to `1000`.
    pub uid: u32,
    /// The group the server switches to when it starts as root, from `PGID`. Defaults to `1000`.
    pub gid: u32,
}

impl Config {
    /// Reads the settings from the environment.
    ///
    /// Returns a message naming the variable when a value can't be used.
    pub fn from_env() -> Result<Self, String> {
        let data_dir = env::var_os("MEWIKI_DATA").map_or_else(|| PathBuf::from("/data"), PathBuf::from);
        let addr = Self::listen_addr()?;
        let password = env::var("MEWIKI_PASSWORD").unwrap_or_default();
        if password.is_empty() {
            return Err(
                "MEWIKI_PASSWORD is not set; the wiki needs a password to protect pages and allow editing".into(),
            );
        }
        let cookie_secure = env::var("MEWIKI_COOKIE_SECURE").map_or(true, |v| v != "false");
        Ok(Config {
            data_dir,
            addr,
            password,
            cookie_secure,
            uid: id_from_env("PUID")?,
            gid: id_from_env("PGID")?,
        })
    }

    /// Reads only the listen address, from `MEWIKI_ADDR`, which is all the `health` command needs.
    pub fn listen_addr() -> Result<SocketAddr, String> {
        match env::var("MEWIKI_ADDR") {
            Ok(value) => value
                .parse()
                .map_err(|e| format!("MEWIKI_ADDR {value:?} is not an address: {e}")),
            Err(_) => Ok(SocketAddr::from(([0, 0, 0, 0], 8080))),
        }
    }
}

fn id_from_env(name: &str) -> Result<u32, String> {
    match env::var(name) {
        Ok(value) => value
            .parse()
            .map_err(|_| format!("{name} {value:?} is not a numeric user or group id")),
        Err(_) => Ok(1000),
    }
}
