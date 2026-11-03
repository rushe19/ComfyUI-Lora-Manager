//! Download queue worker: resumable, hash-verified downloads that land with a LoRA Manager sidecar.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use futures_util::StreamExt;
use lmx_civitai::sidecar::LocalFacts;
use lmx_core::{
    downloads::{DownloadJob, DownloadStore, JobStatus},
    paths,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::{io::AsyncWriteExt, sync::Notify};

use crate::AppState;

pub const PART_SUFFIX: &str = ".lmx-part";

pub fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// Live progress of a running job (not persisted).
#[derive(Default)]
pub struct Live {
    pub bytes: AtomicU64,
    pub cancel: AtomicBool,
}

pub struct Downloads {
    store: Mutex<DownloadStore>,
    jobs: Mutex<Vec<DownloadJob>>,
    live: Mutex<HashMap<i64, Arc<Live>>>,
    pub notify: Notify,
}

impl Downloads {
    pub fn open(path: &Path) -> anyhow::Result<Downloads> {
        let store = DownloadStore::open(path)?;
        let jobs = store.load()?;
        Ok(Downloads { store: Mutex::new(store), jobs: Mutex::new(jobs), live: Mutex::new(HashMap::new()), notify: Notify::new() })
    }

    /// Snapshot of all jobs with live byte counters applied.
    pub fn list(&self) -> Vec<DownloadJob> {
        let live = self.live.lock().expect("live lock");
        self.jobs
            .lock()
            .expect("jobs lock")
            .iter()
            .map(|j| {
                let mut j = j.clone();
                if let Some(l) = live.get(&j.id) {
                    j.bytes_done = l.bytes.load(Ordering::Relaxed);
                }
                j.meta = None;
                j
            })
            .collect()
    }

    /// civitai version id -> status of its most recent job.
    pub fn version_status(&self) -> HashMap<i64, String> {
        self.jobs.lock().expect("jobs lock").iter().map(|j| (j.version_id, j.status.as_str().to_string())).collect()
    }

    pub fn get(&self, id: i64) -> Option<DownloadJob> {
        self.jobs.lock().expect("jobs lock").iter().find(|j| j.id == id).cloned()
    }

    pub fn enqueue(&self, mut job: DownloadJob) -> anyhow::Result<DownloadJob> {
        {
            let jobs = self.jobs.lock().expect("jobs lock");
            if let Some(dup) = jobs.iter().find(|j| j.status.is_active() && j.version_id == job.version_id && j.file_id == job.file_id) {
                anyhow::bail!("already queued (job {})", dup.id);
            }
        }
        job.created_at = now();
        job.updated_at = job.created_at;
        job.status = JobStatus::Queued;
        self.store.lock().expect("store lock").insert(&mut job)?;
        self.jobs.lock().expect("jobs lock").push(job.clone());
        self.notify.notify_one();
        Ok(job)
    }

    fn save(&self, job: &DownloadJob) {
        let mut job = job.clone();
        job.updated_at = now();
        if let Err(e) = self.store.lock().expect("store lock").update(&job) {
            tracing::error!("persist download {}: {e}", job.id);
        }
        let mut jobs = self.jobs.lock().expect("jobs lock");
        if let Some(slot) = jobs.iter_mut().find(|j| j.id == job.id) {
            *slot = job;
        }
    }

    fn set_status(&self, id: i64, status: JobStatus, error: Option<String>) -> Option<DownloadJob> {
        let mut job = self.get(id)?;
        job.status = status;
        job.error = error;
        self.save(&job);
        Some(job)
    }

    /// Cancel an active job (its partial file is removed by the worker) or forget a finished one.
    pub fn cancel_or_remove(&self, id: i64) -> anyhow::Result<&'static str> {
        let job = self.get(id).ok_or_else(|| anyhow::anyhow!("job not found"))?;
        match job.status {
            JobStatus::Running => {
                if let Some(l) = self.live.lock().expect("live lock").get(&id) {
                    l.cancel.store(true, Ordering::SeqCst);
                }
                Ok("canceling")
            }
            JobStatus::Queued => {
                self.set_status(id, JobStatus::Canceled, None);
                let _ = std::fs::remove_file(part_path(&job));
                Ok("canceled")
            }
            _ => {
                self.store.lock().expect("store lock").delete(id)?;
                self.jobs.lock().expect("jobs lock").retain(|j| j.id != id);
                Ok("removed")
            }
        }
    }

    pub fn retry(&self, id: i64) -> anyhow::Result<()> {
        let job = self.get(id).ok_or_else(|| anyhow::anyhow!("job not found"))?;
        anyhow::ensure!(matches!(job.status, JobStatus::Failed | JobStatus::Canceled), "job is {}", job.status.as_str());
        self.set_status(id, JobStatus::Queued, None);
        self.notify.notify_one();
        Ok(())
    }

    fn next_queued(&self) -> Option<i64> {
        self.jobs.lock().expect("jobs lock").iter().find(|j| j.status == JobStatus::Queued).map(|j| j.id)
    }

    fn running(&self) -> usize {
        self.jobs.lock().expect("jobs lock").iter().filter(|j| j.status == JobStatus::Running).count()
    }
}

pub fn dest_path(job: &DownloadJob) -> PathBuf {
    Path::new(&job.dest_dir).join(&job.file_name)
}

pub fn part_path(job: &DownloadJob) -> PathBuf {
    paths::append(&dest_path(job), PART_SUFFIX)
}

/// Start the scheduler: keeps up to `download_concurrency` jobs running.
pub fn spawn_worker(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            while state.downloads.running() < state.cfg.download_concurrency.max(1) {
                let Some(id) = state.downloads.next_queued() else { break };
                state.downloads.set_status(id, JobStatus::Running, None);
                let st = state.clone();
                tokio::spawn(async move {
                    run_job(&st, id).await;
                    st.downloads.notify.notify_one();
                });
            }
            let _ = tokio::time::timeout(Duration::from_secs(5), state.downloads.notify.notified()).await;
        }
    });
}

async fn hash_existing(path: &Path) -> std::io::Result<(Sha256, u64)> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let mut f = std::fs::File::open(&path)?;
        let mut h = Sha256::new();
        let mut buf = vec![0u8; 4 * 1024 * 1024];
        let mut n_total = 0u64;
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
            n_total += n as u64;
        }
        Ok((h, n_total))
    })
    .await
    .map_err(std::io::Error::other)?
}

enum Outcome {
    Done(PathBuf),
    Canceled,
}

pub async fn run_job(state: &Arc<AppState>, id: i64) {
    let live = Arc::new(Live::default());
    state.downloads.live.lock().expect("live lock").insert(id, live.clone());
    let result = download(state, id, &live).await;
    state.downloads.live.lock().expect("live lock").remove(&id);
    match result {
        Ok(Outcome::Done(path)) => {
            if let Some(mut job) = state.downloads.get(id) {
                job.status = JobStatus::Done;
                job.error = None;
                job.bytes_done = live.bytes.load(Ordering::Relaxed);
                job.result_path = Some(paths::norm(&path));
                state.downloads.save(&job);
            }
            tracing::info!("download {id} done: {}", path.display());
            state.request_scan();
        }
        Ok(Outcome::Canceled) => {
            if let Some(job) = state.downloads.set_status(id, JobStatus::Canceled, None) {
                let _ = tokio::fs::remove_file(part_path(&job)).await;
            }
        }
        Err(e) => {
            tracing::warn!("download {id} failed: {e}");
            if let Some(mut job) = state.downloads.get(id) {
                job.status = JobStatus::Failed;
                job.error = Some(e.to_string());
                job.bytes_done = live.bytes.load(Ordering::Relaxed);
                state.downloads.save(&job);
            }
        }
    }
}

fn inside_roots(state: &AppState, job: &DownloadJob) -> bool {
    let dir = paths::norm(Path::new(&job.dest_dir)).to_lowercase();
    state.cfg.roots.get(job.model_type).iter().any(|r| {
        let r = paths::norm(r).to_lowercase();
        dir == r || dir.starts_with(&format!("{r}/"))
    })
}

async fn download(state: &Arc<AppState>, id: i64, live: &Live) -> anyhow::Result<Outcome> {
    let job = state.downloads.get(id).ok_or_else(|| anyhow::anyhow!("job vanished"))?;
    anyhow::ensure!(inside_roots(state, &job), "保存先がモデルフォルダの外です: {}", job.dest_dir);
    let dest = dest_path(&job);
    let part = part_path(&job);
    anyhow::ensure!(!dest.exists(), "同じ名前のファイルが既にあります: {}", paths::norm(&dest));
    tokio::fs::create_dir_all(&job.dest_dir).await?;

    // Resume from an existing partial file.
    let (mut hasher, mut have) = if part.exists() { hash_existing(&part).await? } else { (Sha256::new(), 0) };
    let res = state.civitai.download(&job.url, have).await?;
    let mut file = if have > 0 && res.status().as_u16() == 206 {
        tokio::fs::OpenOptions::new().append(true).open(&part).await?
    } else {
        // Server ignored the range (or fresh start): begin again.
        hasher = Sha256::new();
        have = 0;
        tokio::fs::File::create(&part).await?
    };
    live.bytes.store(have, Ordering::Relaxed);
    if job.size_bytes == 0 {
        if let Some(len) = res.content_length() {
            let mut j = job.clone();
            j.size_bytes = have + len;
            state.downloads.save(&j);
        }
    }

    let mut stream = res.bytes_stream();
    let mut last_save = Instant::now();
    while let Some(chunk) = stream.next().await {
        if live.cancel.load(Ordering::SeqCst) {
            drop(file);
            return Ok(Outcome::Canceled);
        }
        let chunk = chunk.map_err(|e| anyhow::anyhow!("通信が切れました: {e}"))?;
        file.write_all(&chunk).await?;
        hasher.update(&chunk);
        have += chunk.len() as u64;
        live.bytes.store(have, Ordering::Relaxed);
        if last_save.elapsed() > Duration::from_secs(5) {
            if let Some(mut j) = state.downloads.get(id) {
                j.bytes_done = have;
                state.downloads.save(&j);
            }
            last_save = Instant::now();
        }
    }
    file.flush().await?;
    drop(file);

    let digest = hex::encode(hasher.finalize());
    if let Some(expected) = &job.sha256 {
        if !expected.eq_ignore_ascii_case(&digest) {
            let _ = tokio::fs::remove_file(&part).await;
            anyhow::bail!("ハッシュが一致しません（ファイルが壊れている可能性）。期待値 {expected} / 実際 {digest}");
        }
    }
    tokio::fs::rename(&part, &dest).await?;

    // Best effort extras: preview image and LoRA Manager sidecar.
    let preview = match &job.image_url {
        Some(url) => save_preview(state, url, &dest).await.unwrap_or_else(|e| {
            tracing::warn!("preview for {}: {e}", dest.display());
            None
        }),
        None => None,
    };
    if let Err(e) = write_sidecar(&job, &dest, &digest, preview.as_deref()).await {
        tracing::warn!("sidecar for {}: {e}", dest.display());
    }
    Ok(Outcome::Done(dest))
}

fn image_ext(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpeg")
    } else if bytes.starts_with(b"\x89PNG") {
        Some("png")
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

async fn save_preview(state: &AppState, url: &str, model: &Path) -> anyhow::Result<Option<String>> {
    let bytes = state.civitai.fetch_bytes(url).await?;
    let Some(ext) = image_ext(&bytes) else { return Ok(None) };
    let path = paths::append(&paths::stem_path(model), &format!(".preview.{ext}"));
    if path.exists() {
        return Ok(Some(paths::norm(&path)));
    }
    tokio::fs::write(&path, &bytes).await?;
    Ok(Some(paths::norm(&path)))
}

async fn write_sidecar(job: &DownloadJob, model: &Path, sha: &str, preview: Option<&str>) -> anyhow::Result<()> {
    let side = paths::sidecar_path(model);
    if side.exists() {
        return Ok(());
    }
    let meta = job.meta.clone().unwrap_or(Value::Null);
    let version = meta.get("version").cloned().unwrap_or(Value::Null);
    let model_json = meta.get("model").cloned().unwrap_or(Value::Null);
    let file = version
        .get("files")
        .and_then(Value::as_array)
        .and_then(|fs| fs.iter().find(|f| f.get("id").and_then(Value::as_i64) == job.file_id))
        .cloned()
        .unwrap_or(Value::Null);
    let md = tokio::fs::metadata(model).await?;
    let modified = md.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs_f64()).unwrap_or(0.0);
    let stem = model.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let model_path = paths::norm(model);
    let local = LocalFacts {
        file_stem: &stem,
        model_path: &model_path,
        size: md.len(),
        modified,
        sha256: sha,
        preview_path: preview,
        preview_nsfw_level: job.nsfw_level,
        now: now(),
    };
    let map = lmx_civitai::sidecar::build(&version, &model_json, &file, &local);
    let text = serde_json::to_string_pretty(&Value::Object(map))?;
    let tmp = paths::append(&side, ".tmp");
    tokio::fs::write(&tmp, text).await?;
    tokio::fs::rename(&tmp, &side).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_image_types() {
        assert_eq!(image_ext(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpeg"));
        assert_eq!(image_ext(b"\x89PNG\r\n"), Some("png"));
        assert_eq!(image_ext(b"RIFF0000WEBPVP8 "), Some("webp"));
        assert_eq!(image_ext(b"<html>"), None);
    }
}
