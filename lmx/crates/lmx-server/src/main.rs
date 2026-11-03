//! lmx server entry point.

use std::{net::SocketAddr, path::PathBuf};

use lmx_server::{app, AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("LMX_LOG").unwrap_or_else(|_| "info".into()))
        .init();

    let host = std::env::var("LMX_HOST").unwrap_or_else(|_| "0.0.0.0".into());
    let port: u16 = std::env::var("LMX_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(9100);
    let web_dir = PathBuf::from(std::env::var("LMX_WEB_DIR").unwrap_or_else(|_| "web/dist".into()));
    let data_dir = PathBuf::from(std::env::var("LMX_DATA").unwrap_or_else(|_| "data".into()));
    std::fs::create_dir_all(&data_dir)?;

    let started = std::time::Instant::now();
    let cfg = lmx_core::Config::load(&data_dir)?;
    let state = AppState::open(cfg)?;
    tracing::info!("index loaded: {} models in {:.0} ms", state.library().len(), started.elapsed().as_secs_f64() * 1000.0);
    if state.cfg.roots.is_empty() {
        tracing::warn!("no model roots configured: edit {}", data_dir.join("lmx.toml").display());
    }

    let addr: SocketAddr = format!("{host}:{port}").parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("lmx listening on http://{addr} (web: {}, data: {})", web_dir.display(), data_dir.display());
    // Serve right away from the stored index; refresh it in the background.
    state.spawn_scan();
    lmx_server::downloads::spawn_worker(state.clone());
    if !state.civitai.has_key() {
        tracing::warn!("no Civitai API key: set civitai_api_key in lmx.toml (or lm_settings) for favorites and gated downloads");
    }
    axum::serve(listener, app(state, web_dir)).await?;
    Ok(())
}
