//! lmx HTTP server: REST API over the in-memory library, previews/thumbnails, static PWA shell.

pub mod api;
pub mod civitai_api;
pub mod downloads;
pub mod metadata;
pub mod ops;
pub mod watch;
pub mod preview;
pub mod search_prefs;
pub mod state;

use std::{path::PathBuf, sync::Arc};

use axum::{
    http::{header, HeaderValue, StatusCode},
    routing::{delete, get, post},
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
        .route("/models/{id}", get(api::model_detail).patch(ops::edit).delete(ops::delete_model))
        .route("/models/{id}/move", post(ops::move_model))
        .route("/bulk", post(ops::bulk))
        .route("/trash", get(ops::trash_list))
        .route("/trash/{id}/restore", post(ops::trash_restore))
        .route("/duplicates", get(ops::duplicates))
        .route("/metadata/fetch", post(ops::metadata_start))
        .route("/metadata/preview", post(ops::metadata_preview))
        .route("/metadata/job", get(ops::metadata_job).delete(ops::metadata_cancel))
        .route("/folders", get(api::folders))
        .route("/facets", get(api::facets))
        .route("/preview/{id}", get(preview::preview))
        .route("/civitai/search", get(civitai_api::search))
        .route("/civitai/base-models", get(search_prefs::get_base_models).put(search_prefs::put_base_models))
        .route("/civitai/models/{id}", get(civitai_api::model))
        .route("/download-targets", get(civitai_api::targets))
        .route("/downloads", get(civitai_api::list).post(civitai_api::enqueue))
        .route("/downloads/urls", post(civitai_api::enqueue_urls))
        .route("/downloads/{id}", delete(civitai_api::cancel))
        .route("/downloads/{id}/retry", post(civitai_api::retry))
        .fallback(|| async { (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "not found"}))) })
        .with_state(state);

    Router::new().nest("/api", api).merge(sw).fallback_service(static_files).layer(CompressionLayer::new())
}
