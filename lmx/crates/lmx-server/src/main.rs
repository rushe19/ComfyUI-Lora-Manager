//! lmx HTTP server (Phase 0 skeleton): health endpoint + static PWA shell with SPA fallback.

use std::{net::SocketAddr, path::PathBuf};

use axum::{
    http::{header, HeaderValue},
    routing::get,
    Json, Router,
};
use serde::Serialize;
use tower_http::{
    compression::CompressionLayer,
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
};

#[derive(Serialize)]
pub struct Health {
    pub status: &'static str,
    pub name: &'static str,
    pub version: &'static str,
}

async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        name: "lmx",
        version: env!("CARGO_PKG_VERSION"),
    })
}

/// Build the application router. `web_dir` is the built PWA (`web/dist`).
pub fn app(web_dir: PathBuf) -> Router {
    let index = web_dir.join("index.html");
    // Unknown non-API paths fall back to index.html so client-side routes deep-link.
    let static_files = ServeDir::new(&web_dir).fallback(ServeFile::new(index));

    // The service worker must never be served stale, or PWA updates get stuck.
    let sw = Router::new()
        .route_service("/sw.js", ServeFile::new(web_dir.join("sw.js")))
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache"),
        ));

    let api = Router::new()
        .route("/health", get(health))
        .fallback(|| async { (axum::http::StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "not found"}))) });

    Router::new()
        .nest("/api", api)
        .merge(sw)
        .fallback_service(static_files)
        .layer(CompressionLayer::new())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("LMX_LOG").unwrap_or_else(|_| "info".into()))
        .init();

    let host = std::env::var("LMX_HOST").unwrap_or_else(|_| "0.0.0.0".into());
    let port: u16 = std::env::var("LMX_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(9100);
    let web_dir = PathBuf::from(std::env::var("LMX_WEB_DIR").unwrap_or_else(|_| "web/dist".into()));

    let addr: SocketAddr = format!("{host}:{port}").parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("lmx listening on http://{addr} (web: {})", web_dir.display());
    axum::serve(listener, app(web_dir)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn fixture_web_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lmx-web-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), "<!doctype html><title>lmx</title>").unwrap();
        std::fs::write(dir.join("sw.js"), "self.addEventListener('fetch',()=>{})").unwrap();
        dir
    }

    async fn get(path: &str) -> (axum::http::StatusCode, axum::http::HeaderMap, String) {
        let res = app(fixture_web_dir())
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let body = res.into_body().collect().await.unwrap().to_bytes();
        (status, headers, String::from_utf8_lossy(&body).into_owned())
    }

    #[tokio::test]
    async fn health_returns_ok_json() {
        let (status, headers, body) = get("/api/health").await;
        assert_eq!(status, 200);
        assert!(headers[header::CONTENT_TYPE].to_str().unwrap().starts_with("application/json"));
        assert!(body.contains("\"status\":\"ok\""));
    }

    #[tokio::test]
    async fn unknown_api_is_json_404_not_spa_html() {
        let (status, _, body) = get("/api/does-not-exist").await;
        assert_eq!(status, 404);
        assert!(body.contains("not found"));
    }

    #[tokio::test]
    async fn deep_link_falls_back_to_index() {
        let (status, _, body) = get("/loras/some/folder").await;
        assert_eq!(status, 200);
        assert!(body.contains("<title>lmx</title>"));
    }

    #[tokio::test]
    async fn service_worker_is_no_cache() {
        let (status, headers, _) = get("/sw.js").await;
        assert_eq!(status, 200);
        assert_eq!(headers[header::CACHE_CONTROL], "no-cache");
    }
}
