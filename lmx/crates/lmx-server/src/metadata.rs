//! Bulk metadata job: resolve models by sha256 from the local archive and/or Civitai and
//! write LoRA Manager compatible sidecars (backed up first). One job at a time.

use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};

use lmx_civitai::{archive::Archive, client::CivitaiError, merge};
use lmx_core::{
    config::MetadataSource,
    ops,
    paths,
    sidecar::{self, Backup},
    ModelRecord, ModelType,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{downloads::now, AppState};

#[derive(Debug, Clone, Default, Serialize)]
pub struct JobView {
    pub id: u64,
    pub mode: Option<MetadataSource>,
    pub running: bool,
    pub canceled: bool,
    pub started_at: f64,
    pub finished_at: Option<f64>,
    pub total: usize,
    pub done: usize,
    pub current: String,
    /// Resolved from the local archive / from Civitai.
    pub archive_hits: usize,
    pub civitai_hits: usize,
    pub unresolved: usize,
    /// Skipped: damaged sidecar, `skip_metadata_refresh`, recently unresolved.
    pub skipped: usize,
    pub hashed: usize,
    pub errors: usize,
    pub api_requests: usize,
    pub image_requests: usize,
    pub previews_added: usize,
    pub backup_dir: Option<String>,
    pub recent_errors: Vec<String>,
    pub elapsed_secs: f64,
}

#[derive(Default)]
pub struct MetaJobs {
    pub view: Mutex<JobView>,
    pub cancel: AtomicBool,
    next_id: Mutex<u64>,
}

#[derive(Debug, Deserialize, Default)]
pub struct StartBody {
    #[serde(rename = "type")]
    pub model_type: Option<String>,
    pub ids: Option<Vec<i64>>,
    pub folder: Option<String>,
    pub mode: Option<String>,
    /// Only models without Civitai metadata (default true; explicit ids ignore it).
    pub only_unlinked: Option<bool>,
    /// Download a preview for models that have none (default true; never in archive_only).
    pub previews: Option<bool>,
    /// Ask Civitai again for hashes it did not know within the last 7 days.
    pub retry_misses: Option<bool>,
    /// Resolve and count, but write nothing.
    pub dry_run: Option<bool>,
}

/// Hashes Civitai did not know, so repeated runs don't re-ask (data/metadata.sqlite).
struct Misses {
    conn: Mutex<Connection>,
}

impl Misses {
    fn open(path: &Path) -> anyhow::Result<Misses> {
        let conn = Connection::open(path)?;
        conn.execute_batch("CREATE TABLE IF NOT EXISTS civitai_misses (sha TEXT PRIMARY KEY, checked_at REAL NOT NULL);")?;
        Ok(Misses { conn: Mutex::new(conn) })
    }
    fn recent(&self, sha: &str, within: f64) -> bool {
        self.conn
            .lock()
            .expect("misses lock")
            .query_row("SELECT checked_at FROM civitai_misses WHERE sha = ?1", params![sha], |r| r.get::<_, f64>(0))
            .optional()
            .ok()
            .flatten()
            .is_some_and(|t| now() - t < within)
    }
    fn record(&self, sha: &str) {
        let _ = self.conn.lock().expect("misses lock").execute("INSERT OR REPLACE INTO civitai_misses (sha, checked_at) VALUES (?1, ?2)", params![sha, now()]);
    }
    fn clear(&self, sha: &str) {
        let _ = self.conn.lock().expect("misses lock").execute("DELETE FROM civitai_misses WHERE sha = ?1", params![sha]);
    }
}

pub fn select_targets(state: &AppState, b: &StartBody) -> Vec<Arc<ModelRecord>> {
    let lib = state.library();
    if let Some(ids) = &b.ids {
        return ids.iter().filter_map(|id| lib.get(*id)).collect();
    }
    let t = b.model_type.as_deref().and_then(ModelType::parse);
    let folder = b.folder.as_deref().map(|f| f.trim_matches('/').to_lowercase()).filter(|f| !f.is_empty());
    let only_unlinked = b.only_unlinked.unwrap_or(true);
    lib.all()
        .filter(|r| t.is_none_or(|t| r.model_type == t))
        .filter(|r| folder.as_ref().is_none_or(|f| {
            let rf = r.folder.to_lowercase();
            rf == *f || rf.starts_with(&format!("{f}/"))
        }))
        .filter(|r| !only_unlinked || r.civitai_version_id.is_none())
        .cloned()
        .collect()
}

pub fn ensure_archive(state: &Arc<AppState>) -> Option<Arc<Archive>> {
    if let Some(a) = state.archive.read().expect("archive lock").clone() {
        return Some(a);
    }
    let src = state.cfg.archive_db.clone()?;
    let idx = state.cfg.data_dir.join("archive_hash.sqlite");
    match Archive::open(&src, &idx) {
        Ok(a) => {
            tracing::info!("archive {}: {} hashes (rebuilt: {:?})", paths::norm(&src), a.rows, a.rebuilt_secs);
            let a = Arc::new(a);
            *state.archive.write().expect("archive lock") = Some(a.clone());
            Some(a)
        }
        Err(e) => {
            tracing::warn!("archive {}: {e}", paths::norm(&src));
            None
        }
    }
}

pub async fn hash_file(path: PathBuf) -> std::io::Result<String> {
    tokio::task::spawn_blocking(move || {
        use sha2::{Digest, Sha256};
        use std::io::Read;
        let mut f = std::fs::File::open(&path)?;
        let mut h = Sha256::new();
        let mut buf = vec![0u8; 4 * 1024 * 1024];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        Ok(hex::encode(h.finalize()))
    })
    .await
    .map_err(std::io::Error::other)?
}

/// Start a job in the background. Err when one is already running.
pub fn start(state: &Arc<AppState>, b: StartBody) -> anyhow::Result<JobView> {
    let jobs = &state.meta;
    {
        let v = jobs.view.lock().expect("meta lock");
        anyhow::ensure!(!v.running, "メタデータ取得はすでに実行中です");
    }
    let mode = b.mode.as_deref().and_then(MetadataSource::parse).unwrap_or(state.cfg.metadata_source);
    let targets = select_targets(state, &b);
    let id = {
        let mut n = jobs.next_id.lock().expect("id lock");
        *n += 1;
        *n
    };
    let dry = b.dry_run.unwrap_or(false);
    let backup = (!dry).then(|| Backup::new(&state.cfg.data_dir, &format!("metadata-{id}")));
    let view = JobView {
        id,
        mode: Some(mode),
        running: true,
        started_at: now(),
        total: targets.len(),
        backup_dir: backup.as_ref().map(|b| paths::norm(&b.dir)),
        ..Default::default()
    };
    *jobs.view.lock().expect("meta lock") = view.clone();
    jobs.cancel.store(false, Ordering::SeqCst);
    let st = state.clone();
    let previews = b.previews.unwrap_or(true) && mode != MetadataSource::ArchiveOnly;
    let retry = b.retry_misses.unwrap_or(false);
    tokio::spawn(async move {
        let t0 = Instant::now();
        run(&st, targets, mode, previews, retry, dry, backup.as_ref(), t0).await;
        let mut v = st.meta.view.lock().expect("meta lock");
        v.running = false;
        v.finished_at = Some(now());
        v.current.clear();
        v.elapsed_secs = t0.elapsed().as_secs_f64();
        tracing::info!(
            "metadata job {} ({:?}) done in {:.1}s: {} archive, {} civitai, {} unresolved, {} skipped, {} errors, {} api requests",
            v.id, v.mode, v.elapsed_secs, v.archive_hits, v.civitai_hits, v.unresolved, v.skipped, v.errors, v.api_requests
        );
        drop(v);
        if !dry {
            st.request_scan();
        }
    });
    Ok(view)
}

fn bump(state: &AppState, f: impl FnOnce(&mut JobView)) {
    let mut v = state.meta.view.lock().expect("meta lock");
    f(&mut v);
}

enum Found {
    Archive(Value, Value),
    Civitai(Value, Value),
    None,
}

#[allow(clippy::too_many_arguments)]
async fn run(state: &Arc<AppState>, targets: Vec<Arc<ModelRecord>>, mode: MetadataSource, previews: bool, retry: bool, dry: bool, backup: Option<&Backup>, t0: Instant) {
    // Every mode uses the archive (civitai_first as the fallback).
    let archive = {
        let st = state.clone();
        tokio::task::spawn_blocking(move || ensure_archive(&st)).await.ok().flatten()
    };
    if archive.is_none() && mode == MetadataSource::ArchiveOnly {
        bump(state, |v| v.recent_errors.push("アーカイブ DB が見つかりません（archive_db を設定してください）".into()));
    }
    let misses = Misses::open(&state.cfg.data_dir.join("metadata.sqlite")).ok();
    let mut since_scan = 0usize;
    for r in targets {
        if state.meta.cancel.load(Ordering::SeqCst) {
            bump(state, |v| v.canceled = true);
            break;
        }
        bump(state, |v| {
            v.current = r.model_name.clone();
            v.elapsed_secs = t0.elapsed().as_secs_f64();
        });
        match process(state, &r, mode, archive.as_deref(), misses.as_ref(), previews, retry, dry, backup).await {
            Ok(()) => {}
            Err(e) => bump(state, |v| {
                v.errors += 1;
                if v.recent_errors.len() >= 20 {
                    v.recent_errors.remove(0);
                }
                v.recent_errors.push(format!("{}: {e}", r.file_name));
            }),
        }
        bump(state, |v| v.done += 1);
        since_scan += 1;
        if !dry && since_scan >= 500 {
            since_scan = 0;
            state.request_scan();
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn process(
    state: &Arc<AppState>,
    r: &ModelRecord,
    mode: MetadataSource,
    archive: Option<&Archive>,
    misses: Option<&Misses>,
    previews: bool,
    retry: bool,
    dry: bool,
    backup: Option<&Backup>,
) -> anyhow::Result<()> {
    let model_path = PathBuf::from(&r.file_path);
    if r.damage.is_some() || r.sidecar_error.is_some() {
        bump(state, |v| v.skipped += 1);
        return Ok(());
    }
    // Respect LoRA Manager's per-model opt-out.
    if r.has_sidecar {
        if let Ok(Some(s)) = sidecar::read(&paths::sidecar_path(&model_path)) {
            if s.data.get("skip_metadata_refresh").and_then(Value::as_bool) == Some(true) {
                bump(state, |v| v.skipped += 1);
                return Ok(());
            }
        }
    }
    let sha = match &r.sha256 {
        Some(s) => s.clone(),
        None => {
            let s = hash_file(model_path.clone()).await?;
            bump(state, |v| v.hashed += 1);
            s
        }
    };
    if sha == lmx_civitai::archive::EMPTY_SHA256 {
        bump(state, |v| v.skipped += 1);
        return Ok(());
    }

    let from_archive = |a: Option<&Archive>| -> Found {
        let Some(a) = a else { return Found::None };
        match a.lookup(&sha).map(|(m, v)| a.version(m, v)) {
            Some(Ok(Some((ver, model)))) => Found::Archive(ver, model),
            _ => Found::None,
        }
    };
    let ask_civitai = async {
        if misses.is_some_and(|m| !retry && m.recent(&sha, 7.0 * 86400.0)) {
            return Ok::<Found, anyhow::Error>(Found::None);
        }
        bump(state, |v| v.api_requests += 1);
        match state.civitai.by_hash(&sha).await {
            Ok(ver) => {
                let model = match ver.get("modelId").and_then(Value::as_i64) {
                    Some(mid) => {
                        bump(state, |v| v.api_requests += 1);
                        state.civitai.model(mid).await.unwrap_or(Value::Null)
                    }
                    None => Value::Null,
                };
                if let Some(m) = misses {
                    m.clear(&sha);
                }
                Ok(Found::Civitai(ver, model))
            }
            Err(CivitaiError::Http(404, _)) => {
                if let Some(m) = misses {
                    m.record(&sha);
                }
                Ok(Found::None)
            }
            Err(e) => Err(anyhow::anyhow!("Civitai: {e}")),
        }
    };
    let mut archive_consulted = false;
    let found = match mode {
        MetadataSource::ArchiveOnly => {
            archive_consulted = archive.is_some();
            from_archive(archive)
        }
        MetadataSource::ArchiveFirst => {
            archive_consulted = archive.is_some();
            match from_archive(archive) {
                Found::None => ask_civitai.await?,
                f => f,
            }
        }
        MetadataSource::CivitaiFirst => match ask_civitai.await {
            Ok(Found::None) | Err(_) => {
                archive_consulted = archive.is_some();
                from_archive(archive)
            }
            Ok(f) => f,
        },
    };
    let (incoming, source) = match found {
        Found::Archive(v, m) => {
            bump(state, |x| x.archive_hits += 1);
            (merge::archive_payload(&v, &m), "archive_db")
        }
        Found::Civitai(v, m) => {
            bump(state, |x| x.civitai_hits += 1);
            (merge::api_payload(&v, &m), "civitai_api")
        }
        Found::None => {
            bump(state, |x| x.unresolved += 1);
            return Ok(());
        }
    };
    if dry {
        return Ok(());
    }

    // Preview for models that have none.
    let mut preview: Option<(String, i64)> = None;
    if previews && r.preview_path.is_none() {
        if let Some((url, nsfw)) = merge::first_image(&incoming) {
            bump(state, |v| v.image_requests += 1);
            if let Ok(bytes) = state.civitai.fetch_bytes(&lmx_civitai::map::resize_image_url(&url, 450)).await {
                if let Some(ext) = crate::downloads::image_ext(&bytes) {
                    let p = paths::append(&paths::stem_path(&model_path), &format!(".preview.{ext}"));
                    if !p.exists() && std::fs::write(&p, &bytes).is_ok() {
                        bump(state, |v| v.previews_added += 1);
                        preview = Some((paths::norm(&p), nsfw));
                    }
                }
            }
        }
    }

    let _w = state.writes.lock().await;
    let backup = backup.cloned();
    let rec = r.clone();
    let sha = sha.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let model_path = PathBuf::from(&rec.file_path);
        let side = paths::sidecar_path(&model_path);
        let apply = |m: &mut serde_json::Map<String, Value>| {
            if m.get("sha256").and_then(Value::as_str).is_none_or(str::is_empty) {
                m.insert("sha256".into(), Value::String(sha.clone()));
            }
            merge::merge_remote(m, &incoming, source, archive_consulted, now());
            if let Some((p, n)) = &preview {
                m.insert("preview_url".into(), Value::String(p.clone()));
                m.insert("preview_nsfw_level".into(), serde_json::json!(n));
            }
        };
        if side.exists() {
            ops::edit_sidecar(&model_path, backup.as_ref(), false, apply)?;
        } else {
            let md = std::fs::metadata(&model_path)?;
            let modified = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs_f64()).unwrap_or(0.0);
            let mut m = ops::default_sidecar(&model_path, md.len(), modified, &sha, rec.preview_path.as_deref());
            apply(&mut m);
            sidecar::write(&side, &m, None)?;
        }
        Ok(())
    })
    .await??;
    Ok(())
}

pub fn cancel(state: &AppState) -> bool {
    let running = state.meta.view.lock().expect("meta lock").running;
    if running {
        state.meta.cancel.store(true, Ordering::SeqCst);
    }
    running
}
