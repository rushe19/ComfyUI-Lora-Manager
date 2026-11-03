//! Write operations on library models: edit, move/rename, delete to the Recycle Bin (+ restore),
//! bulk actions, duplicates. Sidecars are backed up under `data/backups` before any change.

use std::{
    path::{Path as FsPath, PathBuf},
    sync::Arc,
    time::Duration,
};

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use lmx_core::{
    ops::{self, OpError},
    paths,
    sidecar::Backup,
    ModelRecord, ModelType,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::{api, downloads::now, AppState};

fn err(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({ "error": "op", "message": message.into() }))).into_response()
}

fn op_err(e: OpError) -> Response {
    let code = match e {
        OpError::Exists(_) | OpError::Damaged => StatusCode::CONFLICT,
        OpError::NotFound(_) => StatusCode::NOT_FOUND,
        OpError::Invalid(_) => StatusCode::BAD_REQUEST,
        OpError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    err(code, e.to_string())
}

/// Rescan and wait (bounded) so the response reflects the change.
pub async fn rescan_and_wait(st: &Arc<AppState>) {
    st.request_scan();
    for _ in 0..200 {
        tokio::time::sleep(Duration::from_millis(25)).await;
        if !st.scanning.load(std::sync::atomic::Ordering::SeqCst) && !st.rescan_pending.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }
    }
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct EditBody {
    pub model_name: Option<String>,
    pub tags: Option<Vec<String>>,
    pub notes: Option<String>,
    pub favorite: Option<bool>,
    pub exclude: Option<bool>,
    pub base_model: Option<String>,
    pub trained_words: Option<Vec<String>>,
    pub strength: Option<f64>,
    /// Write even if the sidecar was damaged (rewrites the rescued content = repair).
    #[serde(default)]
    pub repair: bool,
}

pub fn apply_edit(m: &mut Map<String, Value>, b: &EditBody) {
    if let Some(v) = &b.model_name {
        let v = v.trim();
        if !v.is_empty() {
            m.insert("model_name".into(), json!(v));
        }
    }
    if let Some(v) = &b.tags {
        let mut tags: Vec<String> = Vec::new();
        for t in v.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
            if !tags.iter().any(|x| x.eq_ignore_ascii_case(t)) {
                tags.push(t.to_string());
            }
        }
        m.insert("tags".into(), json!(tags));
    }
    if let Some(v) = &b.notes {
        m.insert("notes".into(), json!(v));
    }
    if let Some(v) = b.favorite {
        m.insert("favorite".into(), json!(v));
    }
    if let Some(v) = b.exclude {
        m.insert("exclude".into(), json!(v));
    }
    if let Some(v) = &b.base_model {
        let v = v.trim();
        m.insert("base_model".into(), json!(if v.is_empty() { "Unknown" } else { v }));
    }
    if let Some(words) = &b.trained_words {
        let civ = m.entry("civitai").or_insert_with(|| json!({}));
        if let Some(o) = civ.as_object_mut() {
            o.insert("trainedWords".into(), json!(words.iter().map(|w| w.trim()).filter(|w| !w.is_empty()).collect::<Vec<_>>()));
        }
    }
    if let Some(s) = b.strength {
        let s = (s.clamp(-10.0, 10.0) * 100.0).round() / 100.0;
        let mut tips = m
            .get("usage_tips")
            .and_then(Value::as_str)
            .and_then(|t| serde_json::from_str::<Value>(t).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        tips["strength"] = json!(s);
        m.insert("usage_tips".into(), json!(tips.to_string()));
    }
}

fn record(st: &AppState, id: i64) -> Result<Arc<ModelRecord>, Response> {
    st.library().get(id).ok_or_else(|| err(StatusCode::NOT_FOUND, "モデルが見つかりません"))
}

pub async fn edit(State(st): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<EditBody>) -> Response {
    let r = match record(&st, id) {
        Ok(r) => r,
        Err(e) => return e,
    };
    if !r.has_sidecar {
        return err(StatusCode::CONFLICT, "メタデータファイルがありません。先にメタデータを取得してください");
    }
    let backup = Backup::new(&st.cfg.data_dir, &format!("edit-{id}"));
    let path = PathBuf::from(&r.file_path);
    let _w = st.writes.lock().await;
    let res = tokio::task::spawn_blocking(move || ops::edit_sidecar(&path, Some(&backup), b.repair, |m| apply_edit(m, &b))).await;
    drop(_w);
    match res {
        Ok(Ok(_)) => {
            rescan_and_wait(&st).await;
            api::model_detail(State(st), Path(id)).await
        }
        Ok(Err(e)) => op_err(e),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Debug, Deserialize)]
pub struct MoveBody {
    pub root: Option<String>,
    pub folder: Option<String>,
    /// New file stem (rename). Keeps the folder when `folder` is omitted.
    pub name: Option<String>,
}

fn target_dir(st: &AppState, r: &ModelRecord, root: Option<&str>, folder: Option<&str>) -> anyhow::Result<PathBuf> {
    match folder {
        Some(f) => st.cfg.download_dir(r.model_type, root.or(Some(r.root.as_str())), f),
        None => Ok(FsPath::new(&r.file_path).parent().map(|p| p.to_path_buf()).unwrap_or_default()),
    }
}

pub async fn move_model(State(st): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<MoveBody>) -> Response {
    let r = match record(&st, id) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let dir = match target_dir(&st, &r, b.root.as_deref(), b.folder.as_deref()) {
        Ok(d) => d,
        Err(e) => return err(StatusCode::BAD_REQUEST, e.to_string()),
    };
    let backup = Backup::new(&st.cfg.data_dir, &format!("move-{id}"));
    let from = PathBuf::from(&r.file_path);
    let name = b.name.clone().map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    let _w = st.writes.lock().await;
    let res = tokio::task::spawn_blocking(move || ops::move_model(&from, &dir, name.as_deref(), Some(&backup))).await;
    drop(_w);
    match res {
        Ok(Ok(new_path)) => {
            rescan_and_wait(&st).await;
            let lib = st.library();
            let new_id = lib.all().find(|x| x.file_path.eq_ignore_ascii_case(&paths::norm(&new_path))).map(|x| x.id);
            Json(json!({ "path": paths::norm(&new_path), "id": new_id })).into_response()
        }
        Ok(Err(e)) => op_err(e),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

// ---- Recycle Bin ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrashEntry {
    pub id: u64,
    pub at: f64,
    pub model_name: String,
    pub model_type: ModelType,
    pub paths: Vec<String>,
    #[serde(default)]
    pub restored: bool,
}

fn trash_log_path(st: &AppState) -> PathBuf {
    st.cfg.data_dir.join("trash.json")
}

fn load_trash(st: &AppState) -> Vec<TrashEntry> {
    std::fs::read(trash_log_path(st)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save_trash(st: &AppState, list: &[TrashEntry]) {
    let keep = &list[list.len().saturating_sub(500)..];
    if let Ok(b) = serde_json::to_vec_pretty(keep) {
        let _ = std::fs::write(trash_log_path(st), b);
    }
}

/// Move a model and its companions to the Recycle Bin. Returns the trash log entry.
pub fn trash_model(st: &AppState, r: &ModelRecord) -> anyhow::Result<TrashEntry> {
    let model = PathBuf::from(&r.file_path);
    anyhow::ensure!(model.is_file(), "ファイルが見つかりません: {}", r.file_path);
    let mut files = vec![model.clone()];
    files.extend(ops::companions(&model));
    // Belt and braces: a copy of the sidecar stays in data/backups even after the bin is emptied.
    let _ = Backup::new(&st.cfg.data_dir, &format!("delete-{}", r.id)).save(&paths::sidecar_path(&model));
    trash::delete_all(&files).map_err(|e| anyhow::anyhow!("ゴミ箱へ移動できませんでした: {e}"))?;
    let mut list = load_trash(st);
    let id = list.iter().map(|e| e.id).max().unwrap_or(0) + 1;
    let entry = TrashEntry {
        id,
        at: now(),
        model_name: r.model_name.clone(),
        model_type: r.model_type,
        paths: files.iter().map(|p| paths::norm(p)).collect(),
        restored: false,
    };
    list.push(entry.clone());
    save_trash(st, &list);
    Ok(entry)
}

pub async fn delete_model(State(st): State<Arc<AppState>>, Path(id): Path<i64>) -> Response {
    let r = match record(&st, id) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let _w = st.writes.lock().await;
    let st2 = st.clone();
    let res = tokio::task::spawn_blocking(move || trash_model(&st2, &r)).await;
    drop(_w);
    match res {
        Ok(Ok(entry)) => {
            rescan_and_wait(&st).await;
            Json(json!({ "trash": entry })).into_response()
        }
        Ok(Err(e)) => err(StatusCode::CONFLICT, e.to_string()),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

pub async fn trash_list(State(st): State<Arc<AppState>>) -> Json<Value> {
    let mut list = load_trash(&st);
    list.reverse();
    Json(json!({ "items": list }))
}

pub fn restore_entry(st: &AppState, id: u64) -> anyhow::Result<TrashEntry> {
    let mut list = load_trash(st);
    let entry = list.iter_mut().find(|e| e.id == id).ok_or_else(|| anyhow::anyhow!("履歴が見つかりません"))?;
    anyhow::ensure!(!entry.restored, "すでに元に戻しています");
    for p in &entry.paths {
        anyhow::ensure!(!FsPath::new(p).exists(), "元の場所に同じ名前のファイルがあります: {p}");
    }
    let wanted: Vec<String> = entry.paths.iter().map(|p| p.to_lowercase()).collect();
    let items: Vec<trash::TrashItem> = trash::os_limited::list()
        .map_err(|e| anyhow::anyhow!("ゴミ箱を読めません: {e}"))?
        .into_iter()
        .filter(|i| wanted.contains(&paths::norm(&i.original_path()).to_lowercase()))
        .collect();
    anyhow::ensure!(!items.is_empty(), "ゴミ箱に見つかりません（空にされた可能性があります）");
    // If the same path was deleted more than once, restore only the newest copy of each.
    let mut newest: std::collections::HashMap<String, trash::TrashItem> = std::collections::HashMap::new();
    for i in items {
        let k = paths::norm(&i.original_path()).to_lowercase();
        match newest.get(&k) {
            Some(cur) if cur.time_deleted >= i.time_deleted => {}
            _ => {
                newest.insert(k, i);
            }
        }
    }
    trash::os_limited::restore_all(newest.into_values()).map_err(|e| anyhow::anyhow!("元に戻せませんでした: {e}"))?;
    entry.restored = true;
    let out = entry.clone();
    save_trash(st, &list);
    Ok(out)
}

pub async fn trash_restore(State(st): State<Arc<AppState>>, Path(id): Path<u64>) -> Response {
    let st2 = st.clone();
    match tokio::task::spawn_blocking(move || restore_entry(&st2, id)).await {
        Ok(Ok(e)) => {
            rescan_and_wait(&st).await;
            Json(json!({ "restored": e })).into_response()
        }
        Ok(Err(e)) => err(StatusCode::CONFLICT, e.to_string()),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

// ---- Bulk ----

#[derive(Debug, Deserialize)]
pub struct BulkBody {
    pub ids: Vec<i64>,
    /// favorite | unfavorite | exclude | include | base_model | add_tags | remove_tags | move | delete
    pub action: String,
    pub value: Option<Value>,
    pub root: Option<String>,
    pub folder: Option<String>,
}

pub async fn bulk(State(st): State<Arc<AppState>>, Json(b): Json<BulkBody>) -> Response {
    if b.ids.is_empty() || b.ids.len() > 5000 {
        return err(StatusCode::BAD_REQUEST, "対象の数が不正です");
    }
    let lib = st.library();
    let recs: Vec<Arc<ModelRecord>> = b.ids.iter().filter_map(|id| lib.get(*id)).collect();
    let backup = Backup::new(&st.cfg.data_dir, &format!("bulk-{}", b.action));
    let _w = st.writes.lock().await;
    let st2 = st.clone();
    let res = tokio::task::spawn_blocking(move || {
        let mut ok = 0usize;
        let mut errors: Vec<String> = Vec::new();
        let mut trashed: Vec<u64> = Vec::new();
        for r in &recs {
            let path = PathBuf::from(&r.file_path);
            let result: anyhow::Result<()> = match b.action.as_str() {
                "move" => {
                    let dir = st2.cfg.download_dir(r.model_type, b.root.as_deref().or(Some(r.root.as_str())), b.folder.as_deref().unwrap_or(""));
                    dir.and_then(|d| ops::move_model(&path, &d, None, Some(&backup)).map(|_| ()).map_err(anyhow::Error::from))
                }
                "delete" => trash_model(&st2, r).map(|e| trashed.push(e.id)),
                action => {
                    let tags_of = |v: &Option<Value>| -> Vec<String> {
                        v.as_ref().and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
                    };
                    let edit = match action {
                        "favorite" => EditBody { favorite: Some(true), ..Default::default() },
                        "unfavorite" => EditBody { favorite: Some(false), ..Default::default() },
                        "exclude" => EditBody { exclude: Some(true), ..Default::default() },
                        "include" => EditBody { exclude: Some(false), ..Default::default() },
                        "base_model" => EditBody { base_model: b.value.as_ref().and_then(Value::as_str).map(str::to_string), ..Default::default() },
                        "add_tags" | "remove_tags" => {
                            let change = tags_of(&b.value);
                            let mut tags = r.tags.clone();
                            if action == "add_tags" {
                                tags.extend(change);
                            } else {
                                tags.retain(|t| !change.iter().any(|c| c.eq_ignore_ascii_case(t)));
                            }
                            EditBody { tags: Some(tags), ..Default::default() }
                        }
                        other => {
                            errors.push(format!("unknown action {other}"));
                            break;
                        }
                    };
                    if !r.has_sidecar {
                        Err(anyhow::anyhow!("メタデータファイルがありません"))
                    } else {
                        ops::edit_sidecar(&path, Some(&backup), false, |m| apply_edit(m, &edit)).map(|_| ()).map_err(anyhow::Error::from)
                    }
                }
            };
            match result {
                Ok(()) => ok += 1,
                Err(e) => errors.push(format!("{}: {e}", r.file_name)),
            }
        }
        (ok, errors, trashed)
    })
    .await;
    drop(_w);
    match res {
        Ok((ok, errors, trashed)) => {
            rescan_and_wait(&st).await;
            Json(json!({ "ok": ok, "errors": errors, "trash_ids": trashed })).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

// ---- Duplicates ----

#[derive(Deserialize)]
pub struct DupQuery {
    #[serde(rename = "type")]
    pub model_type: Option<String>,
}

pub async fn duplicates(State(st): State<Arc<AppState>>, Query(q): Query<DupQuery>) -> Json<Value> {
    let t = q.model_type.as_deref().and_then(ModelType::parse);
    let groups = st.library().duplicates(t);
    let wasted: u64 = groups.iter().map(|g| g[0].size * (g.len() as u64 - 1)).sum();
    let out: Vec<Value> = groups
        .iter()
        .map(|g| {
            json!({
                "sha256": g[0].sha256,
                "size": g[0].size,
                "items": g.iter().map(|r| json!({ "summary": api::summary(r), "file_path": r.file_path })).collect::<Vec<_>>(),
            })
        })
        .collect();
    Json(json!({ "groups": out, "wasted_bytes": wasted }))
}

// ---- Metadata job ----

pub async fn metadata_start(State(st): State<Arc<AppState>>, Json(b): Json<crate::metadata::StartBody>) -> Response {
    match crate::metadata::start(&st, b) {
        Ok(v) => (StatusCode::ACCEPTED, Json(json!({ "job": v }))).into_response(),
        Err(e) => err(StatusCode::CONFLICT, e.to_string()),
    }
}

pub async fn metadata_job(State(st): State<Arc<AppState>>) -> Json<Value> {
    let v = st.meta.view.lock().expect("meta lock").clone();
    Json(json!({ "job": if v.id == 0 { Value::Null } else { serde_json::to_value(v).unwrap_or(Value::Null) } }))
}

pub async fn metadata_cancel(State(st): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({ "canceling": crate::metadata::cancel(&st) }))
}

/// Count how many models a job would touch (and how many the archive already knows).
pub async fn metadata_preview(State(st): State<Arc<AppState>>, Json(b): Json<crate::metadata::StartBody>) -> Response {
    let targets = crate::metadata::select_targets(&st, &b);
    let st2 = st.clone();
    let res = tokio::task::spawn_blocking(move || {
        let archive = crate::metadata::ensure_archive(&st2);
        let mut in_archive = 0usize;
        let mut no_hash = 0usize;
        for r in &targets {
            match (&r.sha256, &archive) {
                (Some(s), Some(a)) if a.lookup(s).is_some() => in_archive += 1,
                (None, _) => no_hash += 1,
                _ => {}
            }
        }
        json!({
            "total": targets.len(),
            "in_archive": in_archive,
            "no_hash": no_hash,
            "archive": archive.map(|a| json!({ "path": paths::norm(a.path()), "hashes": a.rows })),
            "default_mode": st2.cfg.metadata_source,
        })
    })
    .await;
    match res {
        Ok(v) => Json(v).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_rules() {
        let mut m: Map<String, Value> = serde_json::from_value(json!({
            "model_name": "a", "tags": ["x"], "usage_tips": "{\"clip_strength\": 1}", "civitai": {"trainedWords": ["w"], "id": 1}
        }))
        .unwrap();
        apply_edit(
            &mut m,
            &EditBody {
                model_name: Some("  ".into()),
                tags: Some(vec![" New ".into(), "new".into(), "".into(), "B".into()]),
                strength: Some(0.756),
                trained_words: Some(vec!["a, b".into(), " ".into()]),
                base_model: Some("".into()),
                ..Default::default()
            },
        );
        assert_eq!(m["model_name"], "a", "blank name ignored");
        assert_eq!(m["tags"], json!(["New", "B"]));
        assert_eq!(m["base_model"], "Unknown");
        assert_eq!(m["civitai"]["trainedWords"], json!(["a, b"]));
        assert_eq!(m["civitai"]["id"], 1);
        let tips: Value = serde_json::from_str(m["usage_tips"].as_str().unwrap()).unwrap();
        assert_eq!(tips["strength"], 0.76);
        assert_eq!(tips["clip_strength"], 1);
    }
}
