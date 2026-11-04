//! New-version check against a fake Civitai `/models?ids=` (no internet).

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use axum::{
    body::Body,
    extract::{Query, State},
    http::Request,
    routing::get,
    Json, Router,
};
use http_body_util::BodyExt;
use lmx_core::{config::Roots, Config};
use lmx_server::{app, AppState};
use serde_json::{json, Value};
use tower::ServiceExt;

/// Model 100: local v10 (Pony); remote has v12 Pony (free), v11 Illustrious, v13 Pony (Buzz).
/// Model 200: local v20, nothing newer. Model 300: deleted on Civitai.
async fn models(State(calls): State<Arc<AtomicUsize>>, Query(q): Query<HashMap<String, String>>) -> Json<Value> {
    calls.fetch_add(1, Ordering::SeqCst);
    let ids: Vec<i64> = q.get("ids").map(|s| s.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or_default();
    let mut items = Vec::new();
    for id in ids {
        match id {
            100 => items.push(json!({"id": 100, "name": "Remote A", "modelVersions": [
                {"id": 13, "name": "v3", "baseModel": "Pony", "publishedAt": "2026-11-03T00:00:00.000Z", "paidAccess": {"permanent": true, "endsAt": null}},
                {"id": 12, "name": "v2", "baseModel": "Pony", "publishedAt": "2026-11-01T00:00:00.000Z", "paidAccess": null},
                {"id": 11, "name": "v2 IL", "baseModel": "Illustrious", "publishedAt": "2026-10-20T00:00:00.000Z"},
                {"id": 10, "name": "v1", "baseModel": "Pony", "publishedAt": "2026-10-01T00:00:00.000Z"}]})),
            200 => items.push(json!({"id": 200, "name": "Remote B", "modelVersions": [{"id": 20, "name": "v1", "baseModel": "SDXL 1.0"}]})),
            _ => {}
        }
    }
    Json(json!({ "items": items, "metadata": {} }))
}

fn sidecar(dir: &std::path::Path, stem: &str, mid: i64, vid: i64, base: &str) {
    std::fs::write(dir.join(format!("{stem}.safetensors")), b"x").unwrap();
    let side = json!({"model_name": stem, "base_model": base, "sha256": format!("{vid:064x}"), "civitai": {"id": vid, "modelId": mid}});
    std::fs::write(dir.join(format!("{stem}.metadata.json")), side.to_string()).unwrap();
}

async fn setup() -> (Arc<AppState>, Arc<AtomicUsize>, std::path::PathBuf, Config) {
    let calls = Arc::new(AtomicUsize::new(0));
    let router = Router::new().route("/api/v1/models", get(models)).with_state(calls.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let base = std::env::temp_dir().join(format!("lmx-updates-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Lora");
    std::fs::create_dir_all(&root).unwrap();
    sidecar(&root, "a", 100, 10, "Pony");
    sidecar(&root, "b", 200, 20, "SDXL 1.0");
    sidecar(&root, "c", 300, 30, "Pony");
    std::fs::write(root.join("unlinked.safetensors"), b"x").unwrap();
    let web = base.join("web");
    std::fs::create_dir_all(&web).unwrap();
    std::fs::write(web.join("index.html"), "<!doctype html>").unwrap();
    std::fs::write(web.join("sw.js"), "").unwrap();
    let cfg = Config {
        roots: Roots { lora: vec![root], ..Default::default() },
        scan_threads: 2,
        data_dir: base.join("data"),
        civitai_api_base: Some(format!("http://{addr}/api/v1")),
        ..Default::default()
    };
    std::fs::create_dir_all(&cfg.data_dir).unwrap();
    let st = AppState::open(cfg.clone()).unwrap();
    assert!(st.spawn_scan());
    while st.scanning.load(Ordering::SeqCst) {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    (st, calls, web, cfg)
}

async fn call(st: &Arc<AppState>, web: &std::path::Path, method: &str, uri: &str, body: Option<Value>) -> Value {
    let req = Request::builder().method(method).uri(uri).header("content-type", "application/json");
    let req = req.body(body.map(|b| Body::from(b.to_string())).unwrap_or_else(Body::empty)).unwrap();
    let res = app(st.clone(), web.to_path_buf()).oneshot(req).await.unwrap();
    assert!(res.status().is_success(), "{uri}: {}", res.status());
    serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

async fn wait_job(st: &Arc<AppState>, web: &std::path::Path) -> Value {
    for _ in 0..200 {
        let s = call(st, web, "GET", "/api/updates/status", None).await;
        if !s["job"]["running"].as_bool().unwrap() {
            return s;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("update check did not finish");
}

#[tokio::test(flavor = "multi_thread")]
async fn check_finds_same_base_updates_and_persists_ignores() {
    let (st, calls, web, cfg) = setup().await;
    let s0 = call(&st, &web, "GET", "/api/updates/status", None).await;
    assert!(s0["last_full_check"].is_null());
    assert_eq!(s0["auto"], true, "daily check is on by default");

    assert_eq!(call(&st, &web, "POST", "/api/updates/check", None).await["started"], true);
    let s = wait_job(&st, &web).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1, "3 linked models -> one batch");
    assert_eq!(s["job"]["total"], 3);
    assert_eq!(s["job"]["gone"], 1, "model 300 is not on Civitai");
    assert!(s["last_full_check"].is_number());
    assert_eq!(s["counts"], json!({"models": 1, "free": 1}));

    let list = call(&st, &web, "GET", "/api/updates", None).await;
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["model_id"], 100);
    let vids: Vec<i64> = items[0]["versions"].as_array().unwrap().iter().map(|v| v["id"].as_i64().unwrap()).collect();
    assert_eq!(vids, [13, 12], "same base only, newest first; Illustrious v11 excluded");
    assert_eq!(items[0]["versions"][0]["access"]["kind"], "paid");
    let free = call(&st, &web, "GET", "/api/updates?free=1", None).await;
    assert_eq!(free["items"][0]["versions"].as_array().unwrap().len(), 1);

    // Library: flag on the card, filter, detail.
    let all = call(&st, &web, "GET", "/api/models?type=lora", None).await;
    let flagged: Vec<&str> = all["items"].as_array().unwrap().iter().filter(|m| m["update"] == true).map(|m| m["name"].as_str().unwrap()).collect();
    assert_eq!(flagged, ["a"]);
    let only = call(&st, &web, "GET", "/api/models?type=lora&updates=1", None).await;
    assert_eq!(only["total"], 1);
    let id = only["items"][0]["id"].as_i64().unwrap();
    let detail = call(&st, &web, "GET", &format!("/api/models/{id}"), None).await;
    assert_eq!(detail["new_versions"].as_array().unwrap().len(), 2);
    assert_eq!(detail["summary"]["update"], true);

    // Ignoring the free version leaves the Buzz one; ignoring the model clears it.
    call(&st, &web, "POST", "/api/updates/ignore", Some(json!({"version_id": 12, "ignore": true}))).await;
    let s = call(&st, &web, "GET", "/api/updates/status", None).await;
    assert_eq!(s["counts"], json!({"models": 1, "free": 0}));
    call(&st, &web, "POST", "/api/updates/ignore", Some(json!({"model_id": 100, "ignore": true}))).await;
    assert_eq!(call(&st, &web, "GET", "/api/updates/status", None).await["counts"]["models"], 0);
    call(&st, &web, "PUT", "/api/updates/auto", Some(json!({"enabled": false}))).await;

    // Stats see the library and the update.
    call(&st, &web, "POST", "/api/updates/ignore", Some(json!({"model_id": 100, "ignore": false}))).await;
    let stats = call(&st, &web, "GET", "/api/stats?type=lora", None).await;
    assert_eq!(stats["total"]["count"], 4);
    assert_eq!(stats["linked"], 3);
    assert_eq!(stats["civitai_models"], 3);
    assert_eq!(stats["updates"], 1);
    assert_eq!(stats["by_base"][0]["name"], "Pony");
    assert_eq!(stats["by_base"][0]["count"], 2);
    call(&st, &web, "POST", "/api/updates/ignore", Some(json!({"model_id": 100, "ignore": true}))).await;

    // A restarted server keeps the results, ignores and the auto setting.
    drop(st);
    let st2 = AppState::open(cfg).unwrap();
    assert!(st2.spawn_scan());
    while st2.scanning.load(Ordering::SeqCst) {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let s = call(&st2, &web, "GET", "/api/updates/status", None).await;
    assert_eq!(s["known_models"], 3);
    assert_eq!(s["ignored_models"], 1);
    assert_eq!(s["ignored_versions"], 1);
    assert_eq!(s["auto"], false);
    call(&st2, &web, "POST", "/api/updates/ignore", Some(json!({"model_id": 100, "ignore": false}))).await;
    assert_eq!(call(&st2, &web, "GET", "/api/updates/status", None).await["counts"], json!({"models": 1, "free": 0}));
}
