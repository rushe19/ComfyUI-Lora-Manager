//! Base models offered as Civitai search filters: the official list from `/api/v1/enums`
//! (cached in `data/civitai_enums.json`, refreshed daily) plus the user's selection,
//! stored server-side in `data/prefs.json` so phone and PC share it.

use std::{path::PathBuf, sync::Arc, time::Duration};

use axum::{extract::State, http::StatusCode, response::IntoResponse, response::Response, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{downloads::now, AppState};

const REFRESH_SECS: f64 = 24.0 * 3600.0;
const NEW_FOR_SECS: f64 = 14.0 * 86400.0;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnumsCache {
    pub fetched_at: f64,
    pub base_models: Vec<String>,
    /// name -> first time lmx saw it (0 = present in the very first fetch).
    #[serde(default)]
    pub first_seen: std::collections::BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Prefs {
    /// Base models shown as chips in Civitai search. `None` = not chosen yet (use local ones).
    #[serde(default)]
    pub search_base_models: Option<Vec<String>>,
    /// Daily new-version check (default on).
    #[serde(default)]
    pub auto_update_check: Option<bool>,
}

#[derive(Default)]
pub struct SearchState {
    pub enums: tokio::sync::Mutex<Option<EnumsCache>>,
    pub prefs: std::sync::Mutex<Option<Prefs>>,
}

fn enums_path(st: &AppState) -> PathBuf {
    st.cfg.data_dir.join("civitai_enums.json")
}

fn prefs_path(st: &AppState) -> PathBuf {
    st.cfg.data_dir.join("prefs.json")
}

pub fn load_prefs(st: &AppState) -> Prefs {
    let mut g = st.search.prefs.lock().expect("prefs lock");
    if g.is_none() {
        *g = Some(std::fs::read(prefs_path(st)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default());
    }
    g.clone().unwrap_or_default()
}

fn save_prefs(st: &AppState, p: &Prefs) -> std::io::Result<()> {
    let path = prefs_path(st);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(p).map_err(std::io::Error::other)?)?;
    std::fs::rename(&tmp, &path)?;
    *st.search.prefs.lock().expect("prefs lock") = Some(p.clone());
    Ok(())
}

/// Change prefs and save them.
pub fn update_prefs(st: &AppState, f: impl FnOnce(&mut Prefs)) -> std::io::Result<()> {
    let mut p = load_prefs(st);
    f(&mut p);
    save_prefs(st, &p)
}

/// Merge a fresh `/enums` list into the cache, recording first-seen times for new names.
pub fn merge_enums(old: Option<&EnumsCache>, fresh: Vec<String>, at: f64) -> EnumsCache {
    let mut first_seen = old.map(|o| o.first_seen.clone()).unwrap_or_default();
    let first_fetch = old.is_none();
    for n in &fresh {
        first_seen.entry(n.clone()).or_insert(if first_fetch { 0.0 } else { at });
    }
    EnumsCache { fetched_at: at, base_models: fresh, first_seen }
}

/// Current list (refreshing when older than a day). Returns (cache, error from the last refresh).
async fn enums(st: &Arc<AppState>) -> (Option<EnumsCache>, Option<String>) {
    let mut g = st.search.enums.lock().await;
    if g.is_none() {
        *g = std::fs::read(enums_path(st)).ok().and_then(|b| serde_json::from_slice(&b).ok());
    }
    let stale = g.as_ref().is_none_or(|c| now() - c.fetched_at > REFRESH_SECS);
    let mut error = None;
    if stale {
        match tokio::time::timeout(Duration::from_secs(20), st.civitai.enums()).await {
            Ok(Ok(v)) => {
                let list: Vec<String> = v
                    .get("ActiveBaseModel")
                    .or_else(|| v.get("BaseModel"))
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                    .unwrap_or_default();
                if !list.is_empty() {
                    let c = merge_enums(g.as_ref(), list, now());
                    if let Ok(b) = serde_json::to_vec_pretty(&c) {
                        let _ = std::fs::write(enums_path(st), b);
                    }
                    *g = Some(c);
                }
            }
            Ok(Err(e)) => error = Some(e.to_string()),
            Err(_) => error = Some("Civitai の応答がありません（タイムアウト）".into()),
        }
    }
    (g.clone(), error)
}

async fn view(st: &Arc<AppState>) -> Value {
    let (cache, error) = enums(st).await;
    let mut all: Vec<String> = cache.as_ref().map(|c| c.base_models.clone()).unwrap_or_default();
    // Local base models (all types) with counts; map to Civitai's spelling when it differs only in case.
    let local: Vec<(String, usize)> = st
        .library()
        .base_models(None)
        .into_iter()
        .filter(|c| !c.name.is_empty() && c.name != "Unknown")
        .map(|c| {
            let canon = all.iter().find(|a| a.eq_ignore_ascii_case(&c.name)).cloned().unwrap_or(c.name);
            (canon, c.count)
        })
        .collect();
    if all.is_empty() {
        all = local.iter().map(|(n, _)| n.clone()).collect();
    }
    let t = now();
    let new: Vec<&String> = cache
        .as_ref()
        .map(|c| c.first_seen.iter().filter(|(_, &s)| s > 0.0 && t - s < NEW_FOR_SECS).map(|(n, _)| n).collect())
        .unwrap_or_default();
    let selected = load_prefs(st).search_base_models.unwrap_or_else(|| {
        let mut l = local.clone();
        l.sort_by(|a, b| b.1.cmp(&a.1));
        l.into_iter().filter(|(n, _)| all.contains(n)).map(|(n, _)| n).collect()
    });
    json!({
        "all": all,
        "selected": selected,
        "new": new,
        "local": local.iter().map(|(n, c)| json!({ "name": n, "count": c })).collect::<Vec<_>>(),
        "fetched_at": cache.as_ref().map(|c| c.fetched_at),
        "error": error,
    })
}

pub async fn get_base_models(State(st): State<Arc<AppState>>) -> Json<Value> {
    Json(view(&st).await)
}

#[derive(Deserialize)]
pub struct SetBody {
    pub selected: Vec<String>,
}

pub async fn put_base_models(State(st): State<Arc<AppState>>, Json(b): Json<SetBody>) -> Response {
    let mut sel: Vec<String> = Vec::new();
    for s in b.selected.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        if !sel.contains(&s) && sel.len() < 200 {
            sel.push(s);
        }
    }
    let mut p = load_prefs(&st);
    p.search_base_models = Some(sel);
    if let Err(e) = save_prefs(&st, &p) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "prefs", "message": e.to_string() }))).into_response();
    }
    Json(view(&st).await).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_fetch_marks_nothing_new_later_fetches_do() {
        let a = merge_enums(None, vec!["Illustrious".into(), "Pony".into()], 100.0);
        assert!(a.first_seen.values().all(|&v| v == 0.0));
        let b = merge_enums(Some(&a), vec!["Illustrious".into(), "Pony".into(), "MOSS Image".into()], 200.0);
        assert_eq!(b.first_seen["MOSS Image"], 200.0);
        assert_eq!(b.first_seen["Pony"], 0.0);
        let c = merge_enums(Some(&b), vec!["MOSS Image".into()], 300.0);
        assert_eq!(c.first_seen["MOSS Image"], 200.0, "first-seen time is kept");
        assert_eq!(c.base_models, vec!["MOSS Image".to_string()]);
    }
}
