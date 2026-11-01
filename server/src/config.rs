//! Runtime configuration, read from environment variables.

use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: SocketAddr,
    pub db_path: PathBuf,
    /// If set, `POST` (sync) requests must carry `Authorization: Bearer <token>`.
    /// This is the same "API token" you type into the HydroCore app.
    pub api_token: Option<String>,
    /// If set, the dashboard and all `GET` endpoints require HTTP Basic auth.
    pub dashboard_user: String,
    pub dashboard_password: Option<String>,
}

fn non_empty(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let bind = non_empty("HYDROCORE_BIND").unwrap_or_else(|| "0.0.0.0:8080".into());
        let bind: SocketAddr = bind
            .parse()
            .map_err(|e| format!("HYDROCORE_BIND={bind:?} is not a valid address: {e}"))?;
        Ok(Config {
            bind,
            db_path: non_empty("HYDROCORE_DB")
                .unwrap_or_else(|| "/data/hydrocore.db".into())
                .into(),
            api_token: non_empty("HYDROCORE_API_TOKEN"),
            dashboard_user: non_empty("HYDROCORE_DASHBOARD_USER").unwrap_or_else(|| "hydro".into()),
            dashboard_password: non_empty("HYDROCORE_DASHBOARD_PASSWORD"),
        })
    }
}
