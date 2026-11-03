//! HTTP-level tests against a temporary library (never the real one).

use std::{path::PathBuf, sync::Arc, time::Duration};

use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use lmx_core::{config::Roots, Config};
use lmx_server::{app, AppState};
use serde_json::Value;
use tower::ServiceExt;

struct Fixture {
    state: Arc<AppState>,
    web: PathBuf,
}

fn fixture(name: &str) -> Fixture {
    let base = std::env::temp_dir().join(format!("lmx-http-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Lora");
    std::fs::create_dir_all(root.join("Char")).unwrap();
    std::fs::write(root.join("Char/miku.safetensors"), b"x").unwrap();
    std::fs::write(
        root.join("Char/miku.metadata.json"),
        r#"{"model_name":"Hatsune Miku","base_model":"Illustrious","usage_tips":"{\"strength\": 0.8}",
            "modelDescription":"<p>desc</p>","tags":["vocaloid"],
            "civitai":{"id":22,"modelId":11,"name":"v1","trainedWords":["hatsune miku"],
                       "images":[{"url":"https://image.civitai.com/a.jpeg","nsfwLevel":1,"type":"image"}]}}"#,
    )
    .unwrap();
    std::fs::write(root.join("Char/miku.webp"), b"RIFF0000WEBP").unwrap();
    std::fs::write(root.join("other.safetensors"), b"x").unwrap();
    let web = base.join("web");
    std::fs::create_dir_all(&web).unwrap();
    std::fs::write(web.join("index.html"), "<!doctype html><title>lmx</title>").unwrap();
    std::fs::write(web.join("sw.js"), "self.addEventListener('fetch',()=>{})").unwrap();
    let cfg = Config {
        roots: Roots { lora: vec![root], ..Default::default() },
        scan_threads: 2,
        civitai_host: "civitai.red".into(),
        data_dir: base.join("data"),
        ..Default::default()
    };
    std::fs::create_dir_all(&cfg.data_dir).unwrap();
    Fixture { state: AppState::open(cfg).unwrap(), web }
}

async fn scan_and_wait(f: &Fixture) {
    assert!(f.state.spawn_scan());
    for _ in 0..200 {
        if !f.state.scanning.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("scan did not finish");
}

async fn get(f: &Fixture, path: &str) -> (u16, String, String) {
    let res = app(f.state.clone(), f.web.clone())
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status().as_u16();
    let ct = res.headers().get("content-type").map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, ct, String::from_utf8_lossy(&body).into_owned())
}

async fn get_json(f: &Fixture, path: &str) -> Value {
    let (status, ct, body) = get(f, path).await;
    assert_eq!(status, 200, "{path}: {body}");
    assert!(ct.starts_with("application/json"), "{path}: content-type {ct}");
    serde_json::from_str(&body).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn list_detail_preview_and_facets() {
    let f = fixture("list");
    scan_and_wait(&f).await;

    let list = get_json(&f, "/api/models?type=loras").await;
    assert_eq!(list["total"], 2);
    let items = list["items"].as_array().unwrap();
    assert_eq!(items[0]["name"], "Hatsune Miku");
    assert_eq!(items[0]["folder"], "Char");
    assert_eq!(items[0]["preview_kind"], "image");
    let id = items[0]["id"].as_i64().unwrap();

    let search = get_json(&f, "/api/models?q=HATSUNE%20miku").await;
    assert_eq!(search["total"], 1);
    let none = get_json(&f, "/api/models?type=checkpoint").await;
    assert_eq!(none["total"], 0);

    let d = get_json(&f, &format!("/api/models/{id}")).await;
    assert_eq!(d["lora_syntax"], "<lora:miku:0.8>");
    assert_eq!(d["trained_words"][0], "hatsune miku");
    assert_eq!(d["civitai_url"], "https://civitai.red/models/11?modelVersionId=22");
    assert_eq!(d["model_description"], "<p>desc</p>");
    assert_eq!(d["images"][0]["url"], "https://image.civitai.com/a.jpeg");

    let preview = items[0]["preview"].as_str().unwrap().to_string();
    let (status, _, body) = get(&f, &preview).await;
    assert_eq!(status, 200);
    assert_eq!(body, "RIFF0000WEBP");
    let (status, ct, _) = get(&f, "/api/preview/999999").await;
    assert_eq!((status, ct.starts_with("application/json")), (404, true));

    let folders = get_json(&f, "/api/folders?type=lora").await;
    assert!(folders.as_array().unwrap().iter().any(|x| x["path"] == "Char" && x["total"] == 1));
    let facets = get_json(&f, "/api/facets?type=lora").await;
    assert!(facets["base_models"].as_array().unwrap().iter().any(|x| x["name"] == "Illustrious"));

    let status = get_json(&f, "/api/status").await;
    assert_eq!(status["counts"]["lora"], 2);
    assert_eq!(status["scanning"], false);
    assert_eq!(status["last_scan"]["stats"]["models"], 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn index_persists_across_restart() {
    let f = fixture("persist");
    scan_and_wait(&f).await;
    let cfg = f.state.cfg.clone();
    let reopened = AppState::open(cfg).unwrap();
    assert_eq!(reopened.library().len(), 2, "loaded from sqlite without scanning");
}

#[tokio::test(flavor = "multi_thread")]
async fn static_shell_and_api_404() {
    let f = fixture("static");
    let (s, _, body) = get(&f, "/loras/deep/link").await;
    assert_eq!(s, 200);
    assert!(body.contains("<title>lmx</title>"));
    let (s, ct, _) = get(&f, "/api/nope").await;
    assert_eq!((s, ct.starts_with("application/json")), (404, true));
    let res = app(f.state.clone(), f.web.clone())
        .oneshot(Request::get("/sw.js").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.headers()["cache-control"], "no-cache");
    let (s, ct, body) = get(&f, "/api/health").await;
    assert_eq!((s, ct.starts_with("application/json")), (200, true));
    assert!(body.contains("\"status\":\"ok\""));
}
