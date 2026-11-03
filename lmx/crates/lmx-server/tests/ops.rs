//! Write operations against a temporary library: metadata job (archive), edit, move, delete/restore,
//! bulk, duplicates. Never touches the real library; Civitai is never contacted (archive_only).

use std::{path::PathBuf, sync::Arc, time::Duration};

use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use lmx_core::{config::Roots, Config};
use lmx_server::{app, AppState};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use tower::ServiceExt;

struct Fx {
    state: Arc<AppState>,
    root: PathBuf,
    web: PathBuf,
}

const ARCH_SHA: &str = "abababababababababababababababababababababababababababababababab";

fn make_archive(path: &std::path::Path) {
    let c = Connection::open(path).unwrap();
    c.execute_batch(
        "CREATE TABLE models (id INTEGER PRIMARY KEY, name TEXT NOT NULL, type TEXT NOT NULL, username TEXT, data TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
         CREATE TABLE model_versions (id INTEGER PRIMARY KEY, model_id INTEGER NOT NULL, position INTEGER NOT NULL, name TEXT NOT NULL, base_model TEXT NOT NULL, published_at INTEGER, data TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
         CREATE TABLE model_files (id INTEGER PRIMARY KEY, model_id INTEGER NOT NULL, version_id INTEGER NOT NULL, type TEXT NOT NULL, sha256 TEXT, data TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);",
    )
    .unwrap();
    c.execute(
        "INSERT INTO models VALUES (10, 'Archived Girl', 'LORA', 'alice', ?1, 0, 0)",
        params![r#"{"id":10,"name":"Archived Girl","description":"<p>desc</p>","tags":["character"],"nsfw":false,"poi":false,"allowNoCredit":true,"allowCommercialUse":["Image"],"allowDerivatives":true,"allowDifferentLicense":true}"#],
    )
    .unwrap();
    c.execute(
        "INSERT INTO model_versions VALUES (20, 10, 0, 'v1', 'Illustrious', 0, ?1, 0, 0)",
        params![r#"{"id":20,"modelId":10,"name":"v1","baseModel":"Illustrious","trainedWords":["archgirl"],"images":[{"url":"https://image.civitai.com/x/y/width=1024/1.jpeg","nsfwLevel":1,"type":"image"}]}"#],
    )
    .unwrap();
    c.execute(
        "INSERT INTO model_files VALUES (30, 10, 20, 'Model', ?1, ?2, 0, 0)",
        params![ARCH_SHA.to_uppercase(), format!(r#"{{"id":30,"name":"a.safetensors","sizeKB":1,"type":"Model","primary":true,"hashes":{{"SHA256":"{}","AutoV3":"ABC"}}}}"#, ARCH_SHA.to_uppercase())],
    )
    .unwrap();
}

fn side(stem: &str, root: &std::path::Path, sha: &str, extra: Value) -> Value {
    let mut v = json!({
        "file_name": stem, "model_name": stem, "file_path": format!("{}/{stem}.safetensors", lmx_core::paths::norm(root)),
        "size": 1, "modified": 1.5, "sha256": sha, "base_model": "Unknown", "preview_url": "", "preview_nsfw_level": 0,
        "notes": "keep me", "from_civitai": true, "civitai": {}, "tags": [], "modelDescription": "", "favorite": false,
        "exclude": false, "metadata_source": null, "last_checked_at": 0, "usage_tips": "{}", "my_unknown_key": [1, 2]
    });
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        for (k, x) in e {
            o.insert(k.clone(), x.clone());
        }
    }
    v
}

fn fixture(name: &str) -> Fx {
    let base = std::env::temp_dir().join(format!("lmx-ops-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Lora");
    std::fs::create_dir_all(root.join("Char")).unwrap();
    // 1) in the archive, sidecar without Civitai data
    std::fs::write(root.join("Char/arch.safetensors"), b"a").unwrap();
    std::fs::write(root.join("Char/arch.metadata.json"), serde_json::to_vec_pretty(&side("arch", &root.join("Char"), ARCH_SHA, json!({}))).unwrap()).unwrap();
    std::fs::write(root.join("Char/arch.preview.png"), b"png").unwrap();
    // 2) no sidecar at all (hashed by the job, not in the archive)
    std::fs::write(root.join("Char/bare.safetensors"), b"bare weights").unwrap();
    // 3) damaged sidecar (skipped)
    std::fs::write(root.join("Char/broken.safetensors"), b"b").unwrap();
    std::fs::write(root.join("Char/broken.metadata.json"), b"{\"file_name\": \"broken\", \"sha256\": \"abababababababababababababababababababababababababababababababab\"}}").unwrap();
    // 4) duplicate of #1 in another folder, opted out of refreshes
    std::fs::create_dir_all(root.join("Dup")).unwrap();
    std::fs::write(root.join("Dup/arch_copy.safetensors"), b"a").unwrap();
    std::fs::write(
        root.join("Dup/arch_copy.metadata.json"),
        serde_json::to_vec_pretty(&side("arch_copy", &root.join("Dup"), ARCH_SHA, json!({"skip_metadata_refresh": true}))).unwrap(),
    )
    .unwrap();
    let archive = base.join("civitai.sqlite");
    make_archive(&archive);
    let web = base.join("web");
    std::fs::create_dir_all(&web).unwrap();
    std::fs::write(web.join("index.html"), "<!doctype html>").unwrap();
    std::fs::write(web.join("sw.js"), "").unwrap();
    let cfg = Config {
        roots: Roots { lora: vec![root.clone()], ..Default::default() },
        scan_threads: 2,
        data_dir: base.join("data"),
        archive_db: Some(archive),
        // Any Civitai call would fail fast instead of reaching the internet.
        civitai_api_base: Some("http://127.0.0.1:9/api/v1".into()),
        ..Default::default()
    };
    std::fs::create_dir_all(&cfg.data_dir).unwrap();
    Fx { state: AppState::open(cfg).unwrap(), root, web }
}

async fn scan(f: &Fx) {
    assert!(f.state.spawn_scan());
    for _ in 0..300 {
        if !f.state.scanning.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("scan did not finish");
}

async fn call(f: &Fx, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let req = Request::builder().method(method).uri(path).header("content-type", "application/json");
    let req = req.body(body.map(|b| Body::from(b.to_string())).unwrap_or_else(Body::empty)).unwrap();
    let res = app(f.state.clone(), f.web.clone()).oneshot(req).await.unwrap();
    let status = res.status().as_u16();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

fn id_of(f: &Fx, file_name: &str) -> i64 {
    f.state.library().all().find(|r| r.file_name == file_name).unwrap_or_else(|| panic!("{file_name} not in library")).id
}

async fn wait_job(f: &Fx) -> Value {
    for _ in 0..500 {
        let (_, v) = call(f, "GET", "/api/metadata/job", None).await;
        if v["job"]["running"] == false {
            return v["job"].clone();
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("job did not finish");
}

#[tokio::test(flavor = "multi_thread")]
async fn archive_only_job_fills_sidecars_without_civitai() {
    let f = fixture("job");
    scan(&f).await;
    let (_, pv) = call(&f, "POST", "/api/metadata/preview", Some(json!({"type": "lora"}))).await;
    assert_eq!(pv["total"], 4);
    assert_eq!(pv["in_archive"], 3, "arch, arch_copy and broken share the archived hash");
    assert_eq!(pv["no_hash"], 1);

    let (st, _) = call(&f, "POST", "/api/metadata/fetch", Some(json!({"type": "lora", "mode": "archive_only"}))).await;
    assert_eq!(st, 202);
    let job = wait_job(&f).await;
    assert_eq!(job["archive_hits"], 1, "{job}");
    assert_eq!(job["api_requests"], 0);
    assert_eq!(job["image_requests"], 0);
    assert_eq!(job["skipped"], 2, "damaged + skip_metadata_refresh");
    assert_eq!(job["unresolved"], 1, "bare is not archived");
    assert_eq!(job["hashed"], 1);
    assert_eq!(job["errors"], 0, "{job}");

    let s: Value = serde_json::from_slice(&std::fs::read(f.root.join("Char/arch.metadata.json")).unwrap()).unwrap();
    assert_eq!(s["model_name"], "Archived Girl");
    assert_eq!(s["base_model"], "Illustrious");
    assert_eq!(s["metadata_source"], "archive_db");
    assert_eq!(s["civitai"]["id"], 20);
    assert_eq!(s["civitai"]["modelId"], 10);
    assert_eq!(s["civitai"]["trainedWords"], json!(["archgirl"]));
    assert_eq!(s["civitai"]["model"]["allowCommercialUse"], json!(["Image"]));
    assert_eq!(s["tags"], json!(["character"]));
    assert_eq!(s["notes"], "keep me");
    assert_eq!(s["my_unknown_key"], json!([1, 2]));
    assert_eq!(s["autov3"], "abc");
    let raw = std::fs::read(f.root.join("Char/arch.metadata.json")).unwrap();
    if cfg!(windows) {
        assert!(raw.windows(2).any(|w| w == b"\r\n"), "CRLF like LoRA Manager");
    }
    // Backup of the original, untouched opt-out and damaged files.
    let backup = PathBuf::from(job["backup_dir"].as_str().unwrap());
    let walked: Vec<_> = walkdir(&backup);
    assert_eq!(walked.len(), 1, "{walked:?}");
    let dup: Value = serde_json::from_slice(&std::fs::read(f.root.join("Dup/arch_copy.metadata.json")).unwrap()).unwrap();
    assert_eq!(dup["metadata_source"], Value::Null);
    assert!(std::fs::read(f.root.join("Char/broken.metadata.json")).unwrap().ends_with(b"}}"));
    assert!(!f.root.join("Char/bare.metadata.json").exists(), "unresolved models get no sidecar");

    // The library reflects the new metadata after the job's rescan.
    tokio::time::sleep(Duration::from_millis(300)).await;
    scan(&f).await;
    let rec = f.state.library().get(id_of(&f, "arch")).unwrap();
    assert_eq!(rec.civitai_version_id, Some(20));
}

fn walkdir(p: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            let path = e.path();
            if path.is_dir() {
                out.extend(walkdir(&path));
            } else {
                out.push(path);
            }
        }
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn edit_move_rename_bulk_and_duplicates() {
    let f = fixture("edit");
    scan(&f).await;
    let id = id_of(&f, "arch");
    let (st, d) = call(&f, "PATCH", &format!("/api/models/{id}"), Some(json!({"favorite": true, "tags": ["a", "b"], "notes": "new", "strength": 0.7}))).await;
    assert_eq!(st, 200, "{d}");
    assert_eq!(d["summary"]["favorite"], true);
    assert_eq!(d["tags"], json!(["a", "b"]));
    assert_eq!(d["notes"], "new");
    assert_eq!(d["strength"], 0.7);
    let s: Value = serde_json::from_slice(&std::fs::read(f.root.join("Char/arch.metadata.json")).unwrap()).unwrap();
    assert_eq!(s["my_unknown_key"], json!([1, 2]));

    // Damaged sidecars are refused unless repair is requested.
    let broken = id_of(&f, "broken");
    let (st, _) = call(&f, "PATCH", &format!("/api/models/{broken}"), Some(json!({"favorite": true}))).await;
    assert_eq!(st, 409);
    let (st, _) = call(&f, "PATCH", &format!("/api/models/{broken}"), Some(json!({"favorite": true, "repair": true}))).await;
    assert_eq!(st, 200);
    assert!(!std::fs::read(f.root.join("Char/broken.metadata.json")).unwrap().ends_with(b"}}"));

    // Move + rename.
    let (st, m) = call(&f, "POST", &format!("/api/models/{id}/move"), Some(json!({"folder": "Anime/Girls", "name": "archived_girl"}))).await;
    assert_eq!(st, 200, "{m}");
    assert!(f.root.join("Anime/Girls/archived_girl.safetensors").exists());
    assert!(f.root.join("Anime/Girls/archived_girl.preview.png").exists());
    assert!(!f.root.join("Char/arch.safetensors").exists());
    let new_id = m["id"].as_i64().unwrap();
    let rec = f.state.library().get(new_id).unwrap();
    assert_eq!(rec.folder, "Anime/Girls");
    assert!(rec.favorite);
    // Escaping the root is refused.
    let (st, _) = call(&f, "POST", &format!("/api/models/{new_id}/move"), Some(json!({"folder": "../../x"}))).await;
    assert_eq!(st, 400);

    // Duplicates: arch(moved) + arch_copy + broken share a hash.
    let (_, dups) = call(&f, "GET", "/api/duplicates?type=lora", None).await;
    assert_eq!(dups["groups"].as_array().unwrap().len(), 1);
    assert_eq!(dups["groups"][0]["items"].as_array().unwrap().len(), 3);

    // Bulk: tag + base model.
    let ids = vec![new_id, id_of(&f, "arch_copy")];
    let (_, b) = call(&f, "POST", "/api/bulk", Some(json!({"ids": ids, "action": "add_tags", "value": ["bulk"]}))).await;
    assert_eq!(b["ok"], 2, "{b}");
    let (_, b) = call(&f, "POST", "/api/bulk", Some(json!({"ids": ids, "action": "base_model", "value": "Pony"}))).await;
    assert_eq!(b["ok"], 2);
    let lib = f.state.library();
    assert!(lib.get(new_id).unwrap().tags.contains(&"bulk".to_string()));
    assert_eq!(lib.get(id_of(&f, "arch_copy")).unwrap().base_model, "Pony");
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_goes_to_recycle_bin_and_restores() {
    let f = fixture("trash");
    scan(&f).await;
    let id = id_of(&f, "arch");
    let (st, d) = call(&f, "DELETE", &format!("/api/models/{id}"), None).await;
    assert_eq!(st, 200, "{d}");
    assert!(!f.root.join("Char/arch.safetensors").exists());
    assert!(!f.root.join("Char/arch.preview.png").exists());
    assert!(f.state.library().all().all(|r| r.file_name != "arch"));
    let tid = d["trash"]["id"].as_u64().unwrap();
    assert_eq!(d["trash"]["paths"].as_array().unwrap().len(), 3);
    let (st, r) = call(&f, "POST", &format!("/api/trash/{tid}/restore"), None).await;
    assert_eq!(st, 200, "{r}");
    assert!(f.root.join("Char/arch.safetensors").exists());
    assert!(f.root.join("Char/arch.metadata.json").exists());
    assert!(f.state.library().all().any(|r| r.file_name == "arch"));
    let (st, _) = call(&f, "POST", &format!("/api/trash/{tid}/restore"), None).await;
    assert_eq!(st, 409, "second restore refused");
}
