//! lmx HTTP server: REST API over the in-memory library, previews/thumbnails, static PWA shell.

pub mod api;
pub mod preview;
pub mod state;

use std::{path::PathBuf, sync::Arc};

use axum::{
    http::{header, HeaderValue, StatusCode},
    routing::{get, post},
    Json, Router,
};
use tower_http::{
    compression::CompressionLayer,
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
};

pub use state::AppState;

/// Build the application router. `web_dir` is the built PWA (`web/dist`).
pub fn app(state: Arc<AppState>, web_dir: PathBuf) -> Router {
    let index = web_dir.join("index.html");
    // Unknown non-API paths fall back to index.html so client-side routes deep-link.
    let static_files = ServeDir::new(&web_dir).fallback(ServeFile::new(index));

    // The service worker must never be served stale, or PWA updates get stuck.
    let sw = Router::new()
        .route_service("/sw.js", ServeFile::new(web_dir.join("sw.js")))
        .layer(SetResponseHeaderLayer::overriding(header::CACHE_CONTROL, HeaderValue::from_static("no-cache")));

    let api = Router::new()
        .route("/health", get(api::health))
        .route("/status", get(api::status))
        .route("/scan", post(api::start_scan))
        .route("/models", get(api::list_models))
        .route("/models/{id}", get(api::model_detail))
        .route("/folders", get(api::folders))
        .route("/facets", get(api::facets))
        .route("/preview/{id}", get(preview::preview))
        .fallback(|| async { (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "not found"}))) })
        .with_state(state);

    Router::new().nest("/api", api).merge(sw).fallback_service(static_files).layer(CompressionLayer::new())
}
