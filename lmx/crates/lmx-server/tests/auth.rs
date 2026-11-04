//! Optional password: off by default, locks /api/* when set, cookie sessions, change/disable.

use std::sync::{atomic::Ordering, Arc};

use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use lmx_core::{config::Roots, Config};
use lmx_server::{app, AppState};
use serde_json::{json, Value};
use tower::ServiceExt;

async fn setup() -> (Arc<AppState>, std::path::PathBuf, Config) {
    let base = std::env::temp_dir().join(format!("lmx-auth-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Lora");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.safetensors"), b"x").unwrap();
    let web = base.join("web");
    std::fs::create_dir_all(&web).unwrap();
    std::fs::write(web.join("index.html"), "<!doctype html>shell").unwrap();
    std::fs::write(web.join("sw.js"), "").unwrap();
    let cfg = Config { roots: Roots { lora: vec![root], ..Default::default() }, scan_threads: 1, data_dir: base.join("data"), ..Default::default() };
    std::fs::create_dir_all(&cfg.data_dir).unwrap();
    let st = AppState::open(cfg.clone()).unwrap();
    assert!(st.spawn_scan());
    while st.scanning.load(Ordering::SeqCst) {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    (st, web, cfg)
}

/// Returns (status, json body or null, Set-Cookie value).
async fn call(st: &Arc<AppState>, web: &std::path::Path, method: &str, uri: &str, body: Option<Value>, cookie: Option<&str>) -> (u16, Value, Option<String>) {
    let mut req = Request::builder().method(method).uri(uri).header("content-type", "application/json");
    if let Some(c) = cookie {
        req = req.header("cookie", c);
    }
    let req = req.body(body.map(|b| Body::from(b.to_string())).unwrap_or_else(Body::empty)).unwrap();
    let res = app(st.clone(), web.to_path_buf()).oneshot(req).await.unwrap();
    let status = res.status().as_u16();
    let set = res.headers().get("set-cookie").map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string());
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null), set)
}

#[tokio::test(flavor = "multi_thread")]
async fn password_locks_the_api_and_sessions_work() {
    let (st, web, cfg) = setup().await;
    // Off by default: everything open.
    assert_eq!(call(&st, &web, "GET", "/api/auth/status", None, None).await.1, json!({"enabled": false, "logged_in": true}));
    assert_eq!(call(&st, &web, "GET", "/api/models?type=lora", None, None).await.0, 200);
    assert_eq!(call(&st, &web, "PUT", "/api/auth/password", Some(json!({"new": "abc"})), None).await.0, 400, "too short");

    // Set a password: the caller gets a session, others are locked out.
    let (s, b, cookie) = call(&st, &web, "PUT", "/api/auth/password", Some(json!({"new": "himitsu-123"})), None).await;
    assert_eq!((s, b), (200, json!({"enabled": true})));
    let cookie = cookie.expect("session cookie");
    assert!(cfg.data_dir.join("auth.json").exists());
    let raw = std::fs::read_to_string(cfg.data_dir.join("auth.json")).unwrap();
    assert!(!raw.contains("himitsu-123"), "only a hash is stored");

    let (s, b, _) = call(&st, &web, "GET", "/api/models?type=lora", None, None).await;
    assert_eq!((s, b["error"].as_str()), (401, Some("auth")));
    assert_eq!(call(&st, &web, "GET", "/api/preview/1", None, None).await.0, 401, "previews are protected too");
    assert_eq!(call(&st, &web, "GET", "/api/health", None, None).await.0, 200);
    assert_eq!(call(&st, &web, "GET", "/api/auth/status", None, None).await.1, json!({"enabled": true, "logged_in": false}));
    assert_eq!(call(&st, &web, "GET", "/api/models?type=lora", None, Some(&cookie)).await.0, 200);
    assert_eq!(call(&st, &web, "GET", "/api/models?type=lora", None, Some("lmx_session=1.deadbeef")).await.0, 401, "forged cookie");
    // The app shell loads without a login so the login screen can show.
    let req = Request::builder().uri("/").body(Body::empty()).unwrap();
    assert_eq!(app(st.clone(), web.clone()).oneshot(req).await.unwrap().status(), 200);

    // Login.
    assert_eq!(call(&st, &web, "POST", "/api/auth/login", Some(json!({"password": "wrong"})), None).await.0, 401);
    let (s, _, c2) = call(&st, &web, "POST", "/api/auth/login", Some(json!({"password": "himitsu-123"})), None).await;
    assert_eq!(s, 200);
    let c2 = c2.unwrap();
    assert_eq!(call(&st, &web, "GET", "/api/models?type=lora", None, Some(&c2)).await.0, 200);

    // Survives a restart.
    drop(st);
    let st = AppState::open(cfg.clone()).unwrap();
    assert_eq!(call(&st, &web, "GET", "/api/models?type=lora", None, Some(&c2)).await.0, 200);

    // Changing it needs the current password and logs out old sessions.
    assert_eq!(call(&st, &web, "PUT", "/api/auth/password", Some(json!({"current": "nope", "new": "second-456"})), Some(&c2)).await.0, 403);
    let (s, _, c3) = call(&st, &web, "PUT", "/api/auth/password", Some(json!({"current": "himitsu-123", "new": "second-456"})), Some(&c2)).await;
    assert_eq!(s, 200);
    assert_eq!(call(&st, &web, "GET", "/api/models?type=lora", None, Some(&c2)).await.0, 401, "old session revoked");
    let c3 = c3.unwrap();
    assert_eq!(call(&st, &web, "GET", "/api/models?type=lora", None, Some(&c3)).await.0, 200);

    // Turning it off.
    let (s, b, _) = call(&st, &web, "PUT", "/api/auth/password", Some(json!({"current": "second-456", "new": ""})), Some(&c3)).await;
    assert_eq!((s, b), (200, json!({"enabled": false})));
    assert!(!cfg.data_dir.join("auth.json").exists());
    assert_eq!(call(&st, &web, "GET", "/api/models?type=lora", None, None).await.0, 200);
}
