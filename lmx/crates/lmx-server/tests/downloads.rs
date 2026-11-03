//! Download pipeline tests against a local HTTP server (no Civitai traffic).

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use axum::Router;
use lmx_core::{config::Roots, downloads::JobStatus, Config};
use lmx_server::{civitai_api::build_job, downloads, AppState};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tower_http::services::ServeDir;

fn civitai_fixture(name: &str) -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../lmx-civitai/tests/fixtures").join(name);
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

struct Env {
    state: Arc<AppState>,
    root: PathBuf,
    base_url: String,
    blob: Vec<u8>,
}

async fn env(name: &str) -> Env {
    let base = std::env::temp_dir().join(format!("lmx-dl-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("Lora");
    let served = base.join("served");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&served).unwrap();
    let blob: Vec<u8> = (0..300_000u32).map(|i| (i * 7 % 251) as u8).collect();
    std::fs::write(served.join("blob.safetensors"), &blob).unwrap();
    std::fs::write(served.join("preview.jpg"), [0xFFu8, 0xD8, 0xFF, 0xE0, 1, 2, 3]).unwrap();

    // ServeDir answers Range requests with 206, like Civitai's storage.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, Router::new().fallback_service(ServeDir::new(served))).await.unwrap() });

    let cfg = Config {
        roots: Roots { lora: vec![root.clone()], ..Default::default() },
        scan_threads: 2,
        data_dir: base.join("data"),
        ..Default::default()
    };
    std::fs::create_dir_all(&cfg.data_dir).unwrap();
    Env { state: AppState::open(cfg).unwrap(), root, base_url: format!("http://{addr}"), blob }
}

fn job_for(e: &Env, sha: String) -> lmx_core::downloads::DownloadJob {
    let model = civitai_fixture("model.json");
    let mut job = build_job(&e.state, &model, None, None, None, "New/Sub").unwrap();
    job.url = format!("{}/blob.safetensors", e.base_url);
    job.image_url = Some(format!("{}/preview.jpg", e.base_url));
    job.sha256 = Some(sha);
    job.file_name = "frieren_test.safetensors".into();
    job
}

#[tokio::test(flavor = "multi_thread")]
async fn resumes_partial_file_verifies_hash_and_writes_sidecar_and_preview() {
    let e = env("resume").await;
    let sha = hex::encode(Sha256::digest(&e.blob));
    let job = e.state.downloads.enqueue(job_for(&e, sha.clone())).unwrap();
    assert!(job.dest_dir.ends_with("Lora/New/Sub"), "{}", job.dest_dir);
    assert_eq!(job.model_type, lmx_core::ModelType::Lora);

    // Simulate an interrupted earlier attempt.
    std::fs::create_dir_all(&job.dest_dir).unwrap();
    std::fs::write(downloads::part_path(&job), &e.blob[..123_456]).unwrap();

    downloads::run_job(&e.state, job.id).await;
    let done = e.state.downloads.get(job.id).unwrap();
    assert_eq!(done.status, JobStatus::Done, "{:?}", done.error);
    let dest = e.root.join("New/Sub/frieren_test.safetensors");
    assert_eq!(std::fs::read(&dest).unwrap(), e.blob);
    assert!(!downloads::part_path(&job).exists());

    let side: Value = serde_json::from_str(&std::fs::read_to_string(e.root.join("New/Sub/frieren_test.metadata.json")).unwrap()).unwrap();
    assert_eq!(side["sha256"], sha);
    assert_eq!(side["metadata_source"], "civitai_api");
    assert_eq!(side["civitai"]["id"], job.version_id);
    assert_eq!(side["civitai"]["modelId"], job.model_id);
    assert!(side["preview_url"].as_str().unwrap().ends_with("frieren_test.preview.jpeg"));
    assert!(e.root.join("New/Sub/frieren_test.preview.jpeg").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn hash_mismatch_fails_and_leaves_no_files() {
    let e = env("badhash").await;
    let job = e.state.downloads.enqueue(job_for(&e, "00".repeat(32))).unwrap();
    downloads::run_job(&e.state, job.id).await;
    let j = e.state.downloads.get(job.id).unwrap();
    assert_eq!(j.status, JobStatus::Failed);
    assert!(j.error.unwrap().contains("ハッシュ"));
    assert!(!e.root.join("New/Sub/frieren_test.safetensors").exists());
    assert!(!downloads::part_path(&job).exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn never_overwrites_existing_file_and_rejects_duplicates() {
    let e = env("exists").await;
    let sha = hex::encode(Sha256::digest(&e.blob));
    let job = e.state.downloads.enqueue(job_for(&e, sha.clone())).unwrap();
    assert!(e.state.downloads.enqueue(job_for(&e, sha)).is_err(), "same version+file twice");
    std::fs::create_dir_all(&job.dest_dir).unwrap();
    std::fs::write(downloads::dest_path(&job), b"user data").unwrap();
    downloads::run_job(&e.state, job.id).await;
    assert_eq!(e.state.downloads.get(job.id).unwrap().status, JobStatus::Failed);
    assert_eq!(std::fs::read(downloads::dest_path(&job)).unwrap(), b"user data");
}

#[tokio::test(flavor = "multi_thread")]
async fn build_job_rejects_folder_escape() {
    let e = env("escape").await;
    let model = civitai_fixture("model.json");
    assert!(build_job(&e.state, &model, None, None, None, "../../outside").is_err());
}
