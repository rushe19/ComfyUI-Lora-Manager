//! Civitai favorites (thumbs-up + notify + bookmark) through the site's tRPC endpoints, against a fake.

use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Mutex},
};

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, Request, StatusCode},
    routing::{get, post},
    Json, Router,
};
use http_body_util::BodyExt;
use lmx_core::{config::Roots, Config};
use lmx_server::{app, AppState};
use serde_json::{json, Value};
use tower::ServiceExt;

#[derive(Clone, Default)]
struct Fake {
    favs: Arc<Mutex<BTreeSet<i64>>>,
    seen_auth: Arc<Mutex<Vec<String>>>,
}

fn authed(h: &HeaderMap, f: &Fake) -> bool {
    let a = h.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    f.seen_auth.lock().unwrap().push(a.clone());
    a == "Bearer test-key"
}

async fn trpc_get(State(f): State<Fake>, h: HeaderMap, Path(proc_): Path<String>, Query(q): Query<HashMap<String, String>>) -> (StatusCode, Json<Value>) {
    assert_eq!(proc_, "user.getEngagedModelsByIds");
    if !authed(&h, &f) {
        return (StatusCode::UNAUTHORIZED, Json(json!({})));
    }
    let input: Value = serde_json::from_str(&q["input"]).unwrap();
    let ids: Vec<i64> = input["json"]["modelIds"].as_array().unwrap().iter().filter_map(Value::as_i64).collect();
    let favs = f.favs.lock().unwrap();
    let rec: Vec<i64> = ids.into_iter().filter(|i| favs.contains(i)).collect();
    // Like the real thing, other engagement kinds are returned too.
    (StatusCode::OK, Json(json!({ "result": { "data": { "json": { "Notify": rec, "Recommended": rec } } } })))
}

async fn trpc_post(State(f): State<Fake>, h: HeaderMap, Path(proc_): Path<String>, Json(b): Json<Value>) -> (StatusCode, Json<Value>) {
    assert_eq!(proc_, "user.toggleFavorite");
    if !authed(&h, &f) {
        return (StatusCode::UNAUTHORIZED, Json(json!({})));
    }
    let id = b["json"]["modelId"].as_i64().unwrap();
    if id == 666 {
        // A key without the social-write scope.
        return (StatusCode::FORBIDDEN, Json(json!({ "error": { "json": { "message": "scope" } } })));
    }
    let on = b["json"]["setTo"].as_bool().unwrap();
    assert!(b["json"].get("modelVersionId").is_none(), "model-level favorite");
    let mut favs = f.favs.lock().unwrap();
    if on {
        favs.insert(id);
    } else {
        favs.remove(&id);
    }
    (StatusCode::OK, Json(json!({ "result": { "data": { "json": true } } })))
}

async fn setup(name: &str, key: Option<&str>) -> (Arc<AppState>, std::path::PathBuf, Fake) {
    let fake = Fake::default();
    fake.favs.lock().unwrap().insert(2843534);
    let router = Router::new().route("/api/trpc/{proc_}", get(trpc_get)).route("/api/trpc/{proc_}", post(trpc_post)).with_state(fake.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let base = std::env::temp_dir().join(format!("lmx-fav-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Lora");
    std::fs::create_dir_all(&root).unwrap();
    let web = base.join("web");
    std::fs::create_dir_all(&web).unwrap();
    std::fs::write(web.join("index.html"), "<!doctype html>").unwrap();
    std::fs::write(web.join("sw.js"), "").unwrap();
    let cfg = Config {
        roots: Roots { lora: vec![root], ..Default::default() },
        scan_threads: 1,
        data_dir: base.join("data"),
        civitai_api_base: Some(format!("http://{addr}/api/v1")),
        civitai_api_key: key.map(str::to_string),
        ..Default::default()
    };
    std::fs::create_dir_all(&cfg.data_dir).unwrap();
    (AppState::open(cfg).unwrap(), web, fake)
}

async fn call(st: &Arc<AppState>, web: &std::path::Path, method: &str, uri: &str, body: Option<Value>) -> (u16, Value) {
    let req = Request::builder().method(method).uri(uri).header("content-type", "application/json");
    let req = req.body(body.map(|b| Body::from(b.to_string())).unwrap_or_else(Body::empty)).unwrap();
    let res = app(st.clone(), web.to_path_buf()).oneshot(req).await.unwrap();
    let s = res.status().as_u16();
    (s, serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap_or(Value::Null))
}

#[tokio::test(flavor = "multi_thread")]
async fn favorite_state_and_toggle_round_trip() {
    let (st, web, fake) = setup("rt", Some("test-key")).await;
    let (s, v) = call(&st, &web, "GET", "/api/civitai/favorites?ids=2843534,5,7", None).await;
    assert_eq!((s, v["favorite"].clone()), (200, json!([2843534])));

    let (s, v) = call(&st, &web, "PUT", "/api/civitai/models/5/favorite", Some(json!({"on": true}))).await;
    assert_eq!((s, v["favorite"].clone()), (200, json!(true)), "{v}");
    assert!(fake.favs.lock().unwrap().contains(&5));
    let (_, v) = call(&st, &web, "GET", "/api/civitai/favorites?ids=5,7,2843534", None).await;
    assert_eq!(v["favorite"], json!([5, 2843534]));

    let (s, v) = call(&st, &web, "PUT", "/api/civitai/models/5/favorite", Some(json!({"on": false}))).await;
    assert_eq!((s, v["favorite"].clone()), (200, json!(false)));
    assert!(!fake.favs.lock().unwrap().contains(&5));
    assert!(fake.seen_auth.lock().unwrap().iter().all(|a| a == "Bearer test-key"));

    // More than 200 ids are split into several requests.
    let many: Vec<String> = (1..=450).map(|i| i.to_string()).collect();
    let before = fake.seen_auth.lock().unwrap().len();
    let (s, _) = call(&st, &web, "GET", &format!("/api/civitai/favorites?ids={}", many.join(",")), None).await;
    assert_eq!(s, 200);
    assert_eq!(fake.seen_auth.lock().unwrap().len() - before, 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_key_and_missing_scope_are_explained() {
    let (st, web, _) = setup("nokey", None).await;
    let (s, v) = call(&st, &web, "PUT", "/api/civitai/models/5/favorite", Some(json!({"on": true}))).await;
    assert_eq!((s, v["error"].as_str()), (400, Some("needs_key")));
    let (s, v) = call(&st, &web, "GET", "/api/civitai/favorites?ids=1", None).await;
    assert_eq!((s, v["error"].as_str()), (400, Some("needs_key")));

    let (st, web, _) = setup("scope", Some("test-key")).await;
    let (s, v) = call(&st, &web, "PUT", "/api/civitai/models/666/favorite", Some(json!({"on": true}))).await;
    assert_eq!((s, v["error"].as_str()), (403, Some("forbidden")));
    assert!(v["message"].as_str().unwrap().contains("権限"), "{v}");
}
