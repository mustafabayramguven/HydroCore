//! HydroCore companion server.

pub mod api;
pub mod auth;
pub mod config;
pub mod db;
pub mod stats;

use axum::extract::DefaultBodyLimit;
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{middleware, Router};
use std::sync::Arc;

pub struct AppState {
    pub config: config::Config,
    pub db: db::Db,
}

const INDEX_HTML: &str = include_str!("../static/index.html");
const APP_JS: &str = include_str!("../static/app.js");
const STYLE_CSS: &str = include_str!("../static/style.css");

async fn asset(content_type: &'static str, body: &'static str) -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, content_type), (header::CACHE_CONTROL, "no-cache")],
        body,
    )
}

pub fn build_app(state: Arc<AppState>) -> Router {
    Router::new()
        // `POST /` is accepted as well, so entering just `http://host:8080` in the app works.
        .route(
            "/",
            get(|| asset("text/html; charset=utf-8", INDEX_HTML)).post(api::sync),
        )
        .route("/app.js", get(|| asset("text/javascript; charset=utf-8", APP_JS)))
        .route("/style.css", get(|| asset("text/css; charset=utf-8", STYLE_CSS)))
        .route("/healthz", get(api::healthz))
        .route("/api/sync", axum::routing::post(api::sync))
        .route("/api/users", get(api::users))
        .route("/api/days", get(api::days))
        .route("/api/days/{date}", get(api::day))
        .route("/api/summary", get(api::summary))
        .route("/api/export.csv", get(api::export_csv))
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), auth::middleware))
        .with_state(state)
}

