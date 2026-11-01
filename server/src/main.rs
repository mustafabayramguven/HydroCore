use hydrocore_server::{build_app, config::Config, db, AppState};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;
use tracing_subscriber::EnvFilter;

/// `hydrocore-server healthcheck`: used by the Docker HEALTHCHECK (the runtime image has
/// no shell, curl or wget).
fn healthcheck(cfg: &Config) -> ExitCode {
    let ip = if cfg.bind.ip().is_unspecified() {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    } else {
        cfg.bind.ip()
    };
    let addr = SocketAddr::new(ip, cfg.bind.port());
    let result = (|| -> std::io::Result<bool> {
        let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(3))?;
        s.set_read_timeout(Some(Duration::from_secs(3)))?;
        s.write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
        let mut buf = String::new();
        s.read_to_string(&mut buf)?;
        Ok(buf.starts_with("HTTP/1.1 200"))
    })();
    if matches!(result, Ok(true)) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
    tracing::info!("shutting down");
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("configuration error: {e}");
            return ExitCode::FAILURE;
        }
    };
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        return healthcheck(&config);
    }

    let conn = match db::open(&config.db_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("cannot open database {}: {e}", config.db_path.display());
            return ExitCode::FAILURE;
        }
    };
    if config.api_token.is_none() {
        tracing::warn!("HYDROCORE_API_TOKEN is not set: anyone who can reach this server can write data");
    }
    if config.dashboard_password.is_none() {
        tracing::warn!("HYDROCORE_DASHBOARD_PASSWORD is not set: the dashboard is open to anyone who can reach it");
    }

    let listener = match tokio::net::TcpListener::bind(config.bind).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("cannot bind {}: {e}", config.bind);
            return ExitCode::FAILURE;
        }
    };
    tracing::info!("HydroCore server {} listening on http://{}  (db: {})", env!("CARGO_PKG_VERSION"), config.bind, config.db_path.display());

    let state = Arc::new(AppState { db: db::Db::new(conn), config });
    if let Err(e) = axum::serve(listener, build_app(state)).with_graceful_shutdown(shutdown_signal()).await {
        tracing::error!("server error: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
