//! Download pipeline tests against a local HTTP server (no Civitai traffic).

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use axum::Router;
use lmx_core::{config::Roots, downloads::JobStatus, Config};
use lmx_server::{civitai_api::build_job, downloads, AppState};
use serde_json::{json, Value};
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
    // The richer /model-versions payload used for the sidecar.
    std::fs::create_dir_all(served.join("api/v1/model-versions")).unwrap();
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../lmx-civitai/tests/fixtures/version.json"),
        served.join("api/v1/model-versions/1336431"),
    )
    .unwrap();

    // ServeDir answers Range requests with 206, like Civitai's storage.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, Router::new().fallback_service(ServeDir::new(served))).await.unwrap() });

    let cfg = Config {
        roots: Roots { lora: vec![root.clone()], ..Default::default() },
        scan_threads: 2,
        data_dir: base.join("data"),
        civitai_api_base: Some(format!("http://{addr}/api/v1")),
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
    // Enriched from /model-versions: example images keep their generation meta.
    assert!(side["civitai"]["images"].as_array().unwrap().iter().any(|i| i.get("meta").is_some()), "image meta missing");
    assert_eq!(side["civitai"]["model"]["name"].as_str().is_some(), true);
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

/// A version whose samples are all videos (video-model LoRAs) still gets a preview: the small
/// transcoded rendition of the first one, stored as `<stem>.preview.mp4` (only that one file).
#[tokio::test(flavor = "multi_thread")]
async fn video_only_version_gets_a_single_mp4_preview() {
    let e = env("videoprev").await;
    // The fake CDN serves an mp4 (ftyp box) at the path the rewritten URL points to.
    let mut mp4 = vec![0, 0, 0, 0x20];
    mp4.extend_from_slice(b"ftypisom");
    mp4.extend_from_slice(&[0u8; 64]);
    std::fs::create_dir_all(e.root.parent().unwrap().join("served/key/uuid/transcode=true,width=450,optimized=true")).unwrap();
    std::fs::write(e.root.parent().unwrap().join("served/key/uuid/transcode=true,width=450,optimized=true/clip.mp4"), &mp4).unwrap();

    let mut model = civitai_fixture("model.json");
    let video = |n: &str| json!({ "url": format!("{}/key/uuid/original=true/{n}", e.base_url), "type": "video", "nsfwLevel": 1 });
    let vs = model["modelVersions"].as_array_mut().unwrap();
    vs[0]["images"] = json!([video("clip.mp4"), video("second.mp4")]);
    let mut job = build_job(&e.state, &model, None, None, None, "Vid").unwrap();
    assert_eq!(
        job.image_url.as_deref(),
        Some(format!("{}/key/uuid/transcode=true,width=450,optimized=true/clip.mp4", e.base_url).as_str()),
        "first video, transcoded rendition"
    );
    job.url = format!("{}/blob.safetensors", e.base_url);
    job.sha256 = Some(hex::encode(Sha256::digest(&e.blob)));
    job.file_name = "vid_lora.safetensors".into();
    let job = e.state.downloads.enqueue(job).unwrap();
    downloads::run_job(&e.state, job.id).await;
    let done = e.state.downloads.get(job.id).unwrap();
    assert_eq!(done.status, JobStatus::Done, "{:?}", done.error);

    assert_eq!(std::fs::read(e.root.join("Vid/vid_lora.preview.mp4")).unwrap(), mp4);
    let mut files: Vec<String> = std::fs::read_dir(e.root.join("Vid")).unwrap().map(|f| f.unwrap().file_name().to_string_lossy().into_owned()).collect();
    files.sort();
    assert_eq!(files, ["vid_lora.metadata.json", "vid_lora.preview.mp4", "vid_lora.safetensors"], "only one sample is downloaded");
    let side: Value = serde_json::from_str(&std::fs::read_to_string(e.root.join("Vid/vid_lora.metadata.json")).unwrap()).unwrap();
    assert!(side["preview_url"].as_str().unwrap().ends_with("vid_lora.preview.mp4"));
}
