//! Keyword + sort search and base-model prefs against a fake Civitai (no internet).

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

#[derive(Clone)]
struct Fake {
    total: usize,
    calls: Arc<AtomicUsize>,
}

/// Relevance order = id order; downloads and dates deliberately scrambled. Like Civitai,
/// `sort` is ignored when `query` is present and the cursor is a numeric offset.
async fn models(State(f): State<Fake>, Query(q): Query<HashMap<String, String>>) -> Json<Value> {
    f.calls.fetch_add(1, Ordering::SeqCst);
    let off: usize = q.get("cursor").and_then(|c| c.parse().ok()).unwrap_or(0);
    let limit: usize = q.get("limit").and_then(|c| c.parse().ok()).unwrap_or(100);
    let items: Vec<Value> = (off..(off + limit).min(f.total))
        .map(|i| {
            let dl = (i * 7919) % 1000;
            let day = (i * 37) % 28 + 1;
            json!({
                "id": i + 1, "name": format!("model {}", i + 1), "type": "LORA", "nsfw": false,
                "stats": {"downloadCount": dl, "thumbsUpCount": (i * 13) % 500},
                "modelVersions": [{"id": 10000 + i, "baseModel": "MOSS Image", "publishedAt": format!("2026-10-{day:02}T00:00:00.000Z"),
                                   "files": [{"name": "x.safetensors", "hashes": {"SHA256": format!("{:064x}", i)}}], "images": []}],
                "description": "long text that the pool drops"
            })
        })
        .collect();
    let next = (off + limit < f.total).then(|| (off + limit).to_string());
    Json(json!({ "items": items, "metadata": { "nextCursor": next } }))
}

async fn enums() -> Json<Value> {
    Json(json!({ "ActiveBaseModel": ["Anima", "Illustrious", "Krea 2", "MOSS Image", "Pony"], "BaseModel": [] }))
}

async fn setup(name: &str, total: usize) -> (Arc<AppState>, Arc<AtomicUsize>, std::path::PathBuf, Config) {
    let calls = Arc::new(AtomicUsize::new(0));
    let fake = Fake { total, calls: calls.clone() };
    let router = Router::new().route("/api/v1/models", get(models)).route("/api/v1/enums", get(enums)).with_state(fake);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let base = std::env::temp_dir().join(format!("lmx-search-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Lora");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.safetensors"), b"x").unwrap();
    std::fs::write(root.join("a.metadata.json"), r#"{"model_name":"a","base_model":"illustrious","sha256":"ff"}"#).unwrap();
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

fn ids_and(v: &Value, key: &str) -> Vec<i64> {
    v["items"].as_array().unwrap().iter().map(|c| c[key].as_i64().unwrap()).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn keyword_search_is_sorted_over_the_whole_pool_and_pages_locally() {
    let (st, calls, web, _) = setup("sort", 250).await;
    let p1 = call(&st, &web, "GET", "/api/civitai/search?type=lora&q=moss&sort=Most%20Downloaded", None).await;
    assert_eq!(p1["resorted"]["pool"], 250);
    assert_eq!(p1["resorted"]["capped"], false);
    assert_eq!(calls.load(Ordering::SeqCst), 3, "100 + 100 + 50");
    let d1 = ids_and(&p1, "downloads");
    assert_eq!(d1.len(), 40);
    assert!(d1.windows(2).all(|w| w[0] >= w[1]), "descending downloads: {d1:?}");
    assert_eq!(p1["next_cursor"], "r40");

    let p2 = call(&st, &web, "GET", "/api/civitai/search?type=lora&q=moss&sort=Most%20Downloaded&cursor=r40", None).await;
    assert_eq!(calls.load(Ordering::SeqCst), 3, "page 2 comes from the cached pool");
    let d2 = ids_and(&p2, "downloads");
    assert!(d1.last().unwrap() >= &d2[0]);
    let all1: std::collections::HashSet<i64> = ids_and(&p1, "id").into_iter().collect();
    assert!(ids_and(&p2, "id").iter().all(|i| !all1.contains(i)), "no overlap");

    // Another sort reuses the same pool.
    let n = call(&st, &web, "GET", "/api/civitai/search?type=lora&q=moss&sort=Newest", None).await;
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(n["items"][0]["base_models"][0], "MOSS Image");

    // Last page has no next cursor.
    let last = call(&st, &web, "GET", "/api/civitai/search?type=lora&q=moss&sort=Newest&cursor=r240", None).await;
    assert_eq!(last["items"].as_array().unwrap().len(), 10);
    assert!(last["next_cursor"].is_null());

    // Without a keyword Civitai sorts itself: pass-through.
    let plain = call(&st, &web, "GET", "/api/civitai/search?type=lora&sort=Newest", None).await;
    assert!(plain["resorted"].is_null());
    assert_eq!(calls.load(Ordering::SeqCst), 4);
}

#[tokio::test(flavor = "multi_thread")]
async fn pool_is_capped_at_1000() {
    let (st, _, web, _) = setup("cap", 1200).await;
    let p = call(&st, &web, "GET", "/api/civitai/search?type=lora&q=x&sort=Highest%20Rated", None).await;
    assert_eq!(p["resorted"]["pool"], 1000);
    assert_eq!(p["resorted"]["capped"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn base_model_prefs_default_to_local_and_persist_on_the_server() {
    let (st, _, web, cfg) = setup("prefs", 10).await;
    let v = call(&st, &web, "GET", "/api/civitai/base-models", None).await;
    assert_eq!(v["all"].as_array().unwrap().len(), 5);
    assert_eq!(v["selected"], json!(["Illustrious"]), "local 'illustrious' mapped to Civitai spelling");
    assert_eq!(v["new"], json!([]), "first fetch marks nothing as new");
    let v = call(&st, &web, "PUT", "/api/civitai/base-models", Some(json!({"selected": ["MOSS Image", " Krea 2 ", "MOSS Image", ""]}))).await;
    assert_eq!(v["selected"], json!(["MOSS Image", "Krea 2"]));
    // A fresh server instance (e.g. after restart, or the phone asking) sees the same selection.
    drop(st);
    let st2 = AppState::open(cfg).unwrap();
    let v = call(&st2, &web, "GET", "/api/civitai/base-models", None).await;
    assert_eq!(v["selected"], json!(["MOSS Image", "Krea 2"]));
}
