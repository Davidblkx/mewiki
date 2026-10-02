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
}

impl Config {
    /// Reads the settings from the environment.
    ///
    /// Returns a message naming the variable when a value can't be used.
    pub fn from_env() -> Result<Self, String> {
        let data_dir = env::var_os("MEWIKI_DATA").map_or_else(|| PathBuf::from("/data"), PathBuf::from);
        let addr = match env::var("MEWIKI_ADDR") {
            Ok(value) => value
                .parse()
                .map_err(|e| format!("MEWIKI_ADDR {value:?} is not an address: {e}"))?,
            Err(_) => SocketAddr::from(([0, 0, 0, 0], 8080)),
        };
        Ok(Config { data_dir, addr })
    }
}
