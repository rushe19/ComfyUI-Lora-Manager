//! New-version check for owned models (LoRA Manager's "same base model" rule).
//!
//! Every Civitai model linked from the library is fetched in batches of 100 (`/models?ids=`),
//! reduced to the few version fields needed, and stored in `data/updates.json`. A local model
//! has an update when Civitai has a version with a higher id and the same base model that is
//! not in the library and not ignored. Early access / Buzz-only versions count but are labelled.

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, RwLock,
    },
    time::Duration,
};

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use lmx_core::{Library, ModelRecord, ModelType};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{api, downloads::now, AppState};

pub const BATCH: usize = 100;
const AUTO_EVERY_SECS: f64 = 24.0 * 3600.0;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemoteVersion {
    pub id: i64,
    pub name: String,
    pub base_model: String,
    #[serde(default)]
    pub published_at: Option<String>,
    /// `{kind: early|paid, ends_at}` as produced by `map::access` at check time.
    #[serde(default)]
    pub access: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RemoteModel {
    pub name: String,
    pub checked_at: f64,
    /// Not returned by Civitai at the last check (deleted or hidden).
    #[serde(default)]
    pub gone: bool,
    pub versions: Vec<RemoteVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Stored {
    pub last_full_check: Option<f64>,
    pub models: HashMap<i64, RemoteModel>,
    #[serde(default)]
    pub ignored_models: HashSet<i64>,
    #[serde(default)]
    pub ignored_versions: HashSet<i64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct JobView {
    pub running: bool,
    pub canceled: bool,
    pub started_at: f64,
    pub finished_at: Option<f64>,
    pub total: usize,
    pub done: usize,
    pub requests: usize,
    pub errors: usize,
    pub gone: usize,
    pub last_error: Option<String>,
    pub auto: bool,
}

/// What the library is missing, recomputed when the library or the stored data changes.
#[derive(Debug, Default)]
pub struct Index {
    /// local model id -> newer versions (newest first)
    pub by_local: HashMap<i64, Vec<RemoteVersion>>,
    /// Civitai model id -> local model id shown for it (the newest local version)
    pub by_model: HashMap<i64, i64>,
}

#[derive(Default)]
pub struct Updates {
    pub stored: RwLock<Stored>,
    pub job: Mutex<JobView>,
    pub cancel: AtomicBool,
    /// (library pointer, stored generation) the index was built for.
    cache: Mutex<Option<(usize, u64, Arc<Index>)>>,
    generation: std::sync::atomic::AtomicU64,
}

fn path(st: &AppState) -> PathBuf {
    st.cfg.data_dir.join("updates.json")
}

impl Updates {
    pub fn load(file: &std::path::Path) -> Updates {
        let stored = std::fs::read(file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Updates { stored: RwLock::new(stored), ..Default::default() }
    }

    fn touch(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    pub fn index(&self, lib: &Arc<Library>) -> Arc<Index> {
        let key = (Arc::as_ptr(lib) as usize, self.generation.load(Ordering::SeqCst));
        let mut c = self.cache.lock().expect("updates cache");
        if let Some((p, g, idx)) = c.as_ref() {
            if (*p, *g) == key {
                return idx.clone();
            }
        }
        let idx = Arc::new(compute(lib, &self.stored.read().expect("updates")));
        *c = Some((key.0, key.1, idx.clone()));
        idx
    }
}

fn norm_base(b: &str) -> String {
    b.trim().to_ascii_lowercase()
}

fn known_base(b: &str) -> bool {
    !b.trim().is_empty() && !b.eq_ignore_ascii_case("unknown")
}

/// Newer versions for one local record, newest first.
pub fn newer_versions(r: &ModelRecord, remote: &RemoteModel, owned: &HashMap<i64, i64>, ignored: &HashSet<i64>) -> Vec<RemoteVersion> {
    let Some(local_v) = r.civitai_version_id else { return Vec::new() };
    let base = norm_base(&r.base_model);
    let mut out: Vec<RemoteVersion> = remote
        .versions
        .iter()
        .filter(|v| v.id > local_v && !owned.contains_key(&v.id) && !ignored.contains(&v.id))
        // Unknown local base: any newer version counts (nothing better to compare with).
        .filter(|v| !known_base(&r.base_model) || norm_base(&v.base_model) == base)
        .cloned()
        .collect();
    out.sort_by(|a, b| b.id.cmp(&a.id));
    out
}

pub fn compute(lib: &Library, s: &Stored) -> Index {
    let owned = lib.version_index();
    let mut idx = Index::default();
    // Only the newest local copy per (Civitai model, base model) is flagged: owning v1 and v2
    // of a model should not put a NEW badge on v1 as well.
    let mut newest: HashMap<(i64, String), &Arc<ModelRecord>> = HashMap::new();
    for r in lib.all() {
        if r.exclude || r.civitai_version_id.is_none() {
            continue;
        }
        let Some(mid) = r.civitai_model_id else { continue };
        let slot = newest.entry((mid, norm_base(&r.base_model))).or_insert(r);
        if slot.civitai_version_id < r.civitai_version_id {
            *slot = r;
        }
    }
    for ((mid, _), r) in newest {
        if s.ignored_models.contains(&mid) {
            continue;
        }
        let Some(remote) = s.models.get(&mid) else { continue };
        let newer = newer_versions(r, remote, owned, &s.ignored_versions);
        if newer.is_empty() {
            continue;
        }
        let keep = match idx.by_model.get(&mid).and_then(|id| lib.get(*id)) {
            Some(prev) => prev.civitai_version_id < r.civitai_version_id,
            None => true,
        };
        if keep {
            idx.by_model.insert(mid, r.id);
        }
        idx.by_local.insert(r.id, newer);
    }
    idx
}

/// Reduce a Civitai model to what the check needs.
pub fn reduce(m: &Value, at: f64) -> RemoteModel {
    let now = lmx_civitai::map::now_iso();
    let versions = m
        .get("modelVersions")
        .and_then(Value::as_array)
        .map(|vs| {
            vs.iter()
                .filter_map(|v| {
                    Some(RemoteVersion {
                        id: v.get("id")?.as_i64()?,
                        name: v.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                        base_model: v.get("baseModel").and_then(Value::as_str).unwrap_or("").to_string(),
                        published_at: v.get("publishedAt").and_then(Value::as_str).map(str::to_string),
                        access: Some(lmx_civitai::map::access(v, &now)).filter(|a| !a.is_null()),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    RemoteModel { name: m.get("name").and_then(Value::as_str).unwrap_or("").to_string(), checked_at: at, gone: false, versions }
}

fn save(st: &AppState) -> std::io::Result<()> {
    let p = path(st);
    let tmp = p.with_extension("json.tmp");
    let bytes = serde_json::to_vec(&*st.updates.stored.read().expect("updates")).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, &p)
}

/// Start a check of every linked model. Returns false if one is running.
pub fn spawn_check(st: &Arc<AppState>, auto: bool) -> bool {
    {
        let mut j = st.updates.job.lock().expect("updates job");
        if j.running {
            return false;
        }
        *j = JobView { running: true, started_at: now(), auto, ..Default::default() };
    }
    st.updates.cancel.store(false, Ordering::SeqCst);
    let st = st.clone();
    tokio::spawn(async move {
        run_check(&st).await;
        let mut j = st.updates.job.lock().expect("updates job");
        j.running = false;
        j.finished_at = Some(now());
    });
    true
}

async fn run_check(st: &Arc<AppState>) {
    let lib = st.library();
    let mut ids: Vec<i64> = lib.all().filter(|r| !r.exclude).filter_map(|r| r.civitai_model_id).collect::<HashSet<_>>().into_iter().collect();
    ids.sort_unstable();
    st.updates.job.lock().expect("updates job").total = ids.len();
    for chunk in ids.chunks(BATCH) {
        if st.updates.cancel.load(Ordering::SeqCst) {
            st.updates.job.lock().expect("updates job").canceled = true;
            break;
        }
        let mut result = st.civitai.models_by_ids(chunk).await;
        if result.is_err() {
            tokio::time::sleep(Duration::from_secs(3)).await;
            result = st.civitai.models_by_ids(chunk).await;
        }
        let at = now();
        let mut j_err = None;
        let mut gone = 0;
        match result {
            Ok(items) => {
                let mut s = st.updates.stored.write().expect("updates");
                let mut seen = HashSet::new();
                for m in &items {
                    if let Some(id) = m.get("id").and_then(Value::as_i64) {
                        seen.insert(id);
                        s.models.insert(id, reduce(m, at));
                    }
                }
                for id in chunk.iter().filter(|i| !seen.contains(i)) {
                    let e = s.models.entry(*id).or_default();
                    e.gone = true;
                    e.checked_at = at;
                    gone += 1;
                }
            }
            Err(e) => j_err = Some(e.to_string()),
        }
        st.updates.touch();
        let mut j = st.updates.job.lock().expect("updates job");
        j.done += chunk.len();
        j.requests += 1;
        j.gone += gone;
        if let Some(e) = j_err {
            j.errors += 1;
            j.last_error = Some(e);
        }
    }
    let (canceled, errors, done) = {
        let j = st.updates.job.lock().expect("updates job");
        (j.canceled, j.errors, j.done)
    };
    // A run counts as a full check unless it was canceled or most batches failed.
    let batches = done.div_ceil(BATCH).max(1);
    if !canceled && errors * 2 <= batches {
        st.updates.stored.write().expect("updates").last_full_check = Some(now());
    }
    if let Err(e) = save(st) {
        tracing::warn!("saving updates.json failed: {e}");
    }
    let idx = st.updates.index(&st.library());
    tracing::info!("update check: {done} models, {} with updates, {errors} failed batches", idx.by_model.len());
}

/// Daily automatic check (when enabled in prefs). First look two minutes after start.
pub fn spawn_scheduler(st: Arc<AppState>) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(120)).await;
        loop {
            let enabled = crate::search_prefs::load_prefs(&st).auto_update_check.unwrap_or(true);
            let last = st.updates.stored.read().expect("updates").last_full_check.unwrap_or(0.0);
            if enabled && !st.scanning.load(Ordering::SeqCst) && now() - last > AUTO_EVERY_SECS {
                spawn_check(&st, true);
            }
            tokio::time::sleep(Duration::from_secs(1800)).await;
        }
    });
}

fn version_json(v: &RemoteVersion) -> Value {
    json!({ "id": v.id, "name": v.name, "base_model": v.base_model, "published_at": v.published_at, "access": v.access })
}

/// Newer versions of one local model, for the detail sheet.
pub fn for_local(st: &AppState, lib: &Arc<Library>, id: i64) -> Value {
    let idx = st.updates.index(lib);
    match idx.by_local.get(&id) {
        Some(vs) => Value::Array(vs.iter().map(version_json).collect()),
        None => json!([]),
    }
}

fn counts(idx: &Index, lib: &Library, t: Option<ModelType>) -> Value {
    let mut models = 0;
    let mut free = 0;
    for (mid, local) in &idx.by_model {
        let Some(r) = lib.get(*local) else { continue };
        if t.is_some_and(|t| t != r.model_type) {
            continue;
        }
        let _ = mid;
        models += 1;
        if idx.by_local.get(local).is_some_and(|vs| vs.iter().any(|v| v.access.is_none())) {
            free += 1;
        }
    }
    json!({ "models": models, "free": free })
}

pub async fn status(State(st): State<Arc<AppState>>) -> Json<Value> {
    let lib = st.library();
    let idx = st.updates.index(&lib);
    let s = st.updates.stored.read().expect("updates");
    let job = st.updates.job.lock().expect("updates job").clone();
    let mut by_type = serde_json::Map::new();
    for t in ModelType::ALL {
        by_type.insert(t.as_str().into(), counts(&idx, &lib, Some(t)));
    }
    Json(json!({
        "job": job,
        "last_full_check": s.last_full_check,
        "known_models": s.models.len(),
        "gone": s.models.values().filter(|m| m.gone).count(),
        "ignored_models": s.ignored_models.len(),
        "ignored_versions": s.ignored_versions.len(),
        "counts": counts(&idx, &lib, None),
        "by_type": by_type,
        "auto": crate::search_prefs::load_prefs(&st).auto_update_check.unwrap_or(true),
    }))
}

pub async fn start(State(st): State<Arc<AppState>>) -> Json<Value> {
    let started = spawn_check(&st, false);
    Json(json!({ "started": started }))
}

pub async fn cancel(State(st): State<Arc<AppState>>) -> Json<Value> {
    st.updates.cancel.store(true, Ordering::SeqCst);
    Json(json!({ "ok": true }))
}

#[derive(Deserialize, Default)]
pub struct ListQuery {
    #[serde(rename = "type")]
    pub model_type: Option<String>,
    /// Only updates that can be downloaded for free right now.
    pub free: Option<u8>,
}

pub async fn list(State(st): State<Arc<AppState>>, Query(q): Query<ListQuery>) -> Json<Value> {
    let lib = st.library();
    let idx = st.updates.index(&lib);
    let t = q.model_type.as_deref().and_then(ModelType::parse);
    let only_free = q.free.unwrap_or(0) == 1;
    let s = st.updates.stored.read().expect("updates");
    let mut rows: Vec<(String, Value)> = Vec::new();
    for (mid, local) in &idx.by_model {
        let Some(r) = lib.get(*local) else { continue };
        if t.is_some_and(|t| t != r.model_type) {
            continue;
        }
        let vs: Vec<&RemoteVersion> = idx.by_local[local].iter().filter(|v| !only_free || v.access.is_none()).collect();
        let Some(newest) = vs.first() else { continue };
        let sort_key = newest.published_at.clone().unwrap_or_default();
        let mut local_card = api::summary(&r);
        local_card.update = true;
        rows.push((
            sort_key,
            json!({
                "model_id": mid,
                "remote_name": s.models.get(mid).map(|m| m.name.as_str()),
                "local": local_card,
                "local_version_id": r.civitai_version_id,
                "versions": vs.iter().map(|v| version_json(v)).collect::<Vec<_>>(),
            }),
        ));
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    Json(json!({ "items": rows.into_iter().map(|r| r.1).collect::<Vec<_>>() }))
}

#[derive(Deserialize)]
pub struct IgnoreBody {
    pub model_id: Option<i64>,
    pub version_id: Option<i64>,
    pub ignore: bool,
}

pub async fn ignore(State(st): State<Arc<AppState>>, Json(b): Json<IgnoreBody>) -> Response {
    {
        let mut s = st.updates.stored.write().expect("updates");
        match (b.model_id, b.version_id) {
            (_, Some(v)) => {
                if b.ignore {
                    s.ignored_versions.insert(v);
                } else {
                    s.ignored_versions.remove(&v);
                }
            }
            (Some(m), None) => {
                if b.ignore {
                    s.ignored_models.insert(m);
                } else {
                    s.ignored_models.remove(&m);
                }
            }
            (None, None) => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "model_id or version_id" }))).into_response(),
        }
    }
    st.updates.touch();
    if let Err(e) = save(&st) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response();
    }
    Json(json!({ "ok": true })).into_response()
}

#[derive(Deserialize)]
pub struct AutoBody {
    pub enabled: bool,
}

pub async fn set_auto(State(st): State<Arc<AppState>>, Json(b): Json<AutoBody>) -> Response {
    match crate::search_prefs::update_prefs(&st, |p| p.auto_update_check = Some(b.enabled)) {
        Ok(()) => Json(json!({ "auto": b.enabled })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rv(id: i64, base: &str) -> RemoteVersion {
        RemoteVersion { id, name: format!("v{id}"), base_model: base.into(), published_at: None, access: None }
    }

    fn rec(id: i64, mid: i64, vid: i64, base: &str) -> ModelRecord {
        let path = format!("H:/L/m{id}.safetensors");
        let facts = lmx_core::record::FileFacts {
            model_type: ModelType::Lora,
            path: std::path::Path::new(&path),
            root: std::path::Path::new("H:/L"),
            size: 1,
            modified: 0.0,
            sidecar_mtime: None,
            sidecar_size: None,
        };
        let mut r = ModelRecord::build(&facts, None, None);
        r.id = id;
        r.civitai_model_id = Some(mid);
        r.civitai_version_id = Some(vid);
        r.base_model = base.into();
        r
    }

    #[test]
    fn same_base_newer_and_not_owned() {
        let remote = RemoteModel { versions: vec![rv(30, "Illustrious"), rv(25, "Pony"), rv(20, "Illustrious"), rv(10, "Illustrious")], ..Default::default() };
        let owned: HashMap<i64, i64> = [(20, 1)].into_iter().collect();
        let r = rec(1, 7, 20, "illustrious");
        let v = newer_versions(&r, &remote, &owned, &HashSet::new());
        assert_eq!(v.iter().map(|x| x.id).collect::<Vec<_>>(), [30], "Pony version and older ones excluded");
        let ignored: HashSet<i64> = [30].into_iter().collect();
        assert!(newer_versions(&r, &remote, &owned, &ignored).is_empty());
        // Owning the new version clears it.
        let owned2: HashMap<i64, i64> = [(20, 1), (30, 2)].into_iter().collect();
        assert!(newer_versions(&r, &remote, &owned2, &HashSet::new()).is_empty());
        // Unknown local base: any newer version.
        let r2 = rec(1, 7, 20, "Unknown");
        assert_eq!(newer_versions(&r2, &remote, &owned, &HashSet::new()).len(), 2);
    }

    #[test]
    fn compute_picks_newest_local_and_respects_model_ignore() {
        let lib = Library::new(vec![rec(1, 7, 10, "Pony"), rec(2, 7, 20, "Pony"), rec(3, 8, 5, "SDXL 1.0")]);
        let mut s = Stored::default();
        s.models.insert(7, RemoteModel { versions: vec![rv(40, "Pony"), rv(20, "Pony"), rv(10, "Pony")], ..Default::default() });
        s.models.insert(8, RemoteModel { versions: vec![rv(6, "SDXL 1.0"), rv(5, "SDXL 1.0")], ..Default::default() });
        let idx = compute(&lib, &s);
        assert_eq!(idx.by_model[&7], 2, "newest local copy represents the model");
        assert!(!idx.by_local.contains_key(&1), "older local copy of the same base is not flagged");
        assert_eq!(idx.by_local[&2].iter().map(|v| v.id).collect::<Vec<_>>(), [40]);
        assert!(idx.by_model.contains_key(&8));
        s.ignored_models.insert(8);
        assert!(!compute(&lib, &s).by_model.contains_key(&8));
    }

    #[test]
    fn reduce_keeps_access() {
        let m = json!({"id": 7, "name": "X", "modelVersions": [
            {"id": 2, "name": "v2", "baseModel": "Pony", "publishedAt": "2026-11-01T00:00:00.000Z", "paidAccess": {"permanent": true}},
            {"id": 1, "name": "v1", "baseModel": "Pony", "paidAccess": null, "images": [1, 2, 3]}]});
        let r = reduce(&m, 5.0);
        assert_eq!(r.versions.len(), 2);
        assert_eq!(r.versions[0].access.as_ref().unwrap()["kind"], "paid");
        assert!(r.versions[1].access.is_none());
    }
}
