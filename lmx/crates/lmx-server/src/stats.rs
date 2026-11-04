//! Library statistics for the stats screen (computed from the in-memory index; ~ms).

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use axum::{
    extract::{Query, State},
    Json,
};
use lmx_core::{Library, ModelType};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{downloads::now, AppState};

#[derive(Default, Clone, Copy)]
struct Acc {
    count: usize,
    size: u64,
}

impl Acc {
    fn add(&mut self, size: u64) {
        self.count += 1;
        self.size += size;
    }
    fn json(self) -> Value {
        json!({ "count": self.count, "size": self.size })
    }
}

fn top(map: HashMap<String, Acc>, n: usize, by_size: bool) -> Vec<Value> {
    let mut v: Vec<(String, Acc)> = map.into_iter().collect();
    v.sort_by(|a, b| {
        let k = if by_size { b.1.size.cmp(&a.1.size) } else { b.1.count.cmp(&a.1.count) };
        k.then_with(|| a.0.cmp(&b.0))
    });
    v.into_iter().take(n).map(|(name, a)| json!({ "name": name, "count": a.count, "size": a.size })).collect()
}

#[derive(Deserialize, Default)]
pub struct StatsQuery {
    #[serde(rename = "type")]
    pub model_type: Option<String>,
}

pub fn compute(lib: &Library, t: Option<ModelType>, updates: usize, at: f64) -> Value {
    let mut total = Acc::default();
    let mut by_type: HashMap<String, Acc> = HashMap::new();
    let mut by_base: HashMap<String, Acc> = HashMap::new();
    let mut by_folder: HashMap<String, Acc> = HashMap::new();
    let (mut linked, mut previews, mut sidecars, mut damaged, mut favorites, mut excluded) = (0, 0, 0, 0, 0, 0);
    let mut by_sha: HashMap<&str, Vec<u64>> = HashMap::new();
    let (mut d7, mut d30) = (Acc::default(), Acc::default());
    let mut largest: Vec<(u64, i64, &str)> = Vec::new();
    let mut creators: HashMap<String, Acc> = HashMap::new();
    let mut seen_models = HashSet::new();
    for r in lib.all() {
        if t.is_some_and(|t| t != r.model_type) {
            continue;
        }
        if r.exclude {
            excluded += 1;
            continue;
        }
        total.add(r.size);
        by_type.entry(r.model_type.as_str().into()).or_default().add(r.size);
        by_base.entry(if r.base_model.is_empty() { "Unknown".into() } else { r.base_model.clone() }).or_default().add(r.size);
        let top_folder = r.folder.split('/').next().unwrap_or("").to_string();
        by_folder.entry(if top_folder.is_empty() { "(ルート)".into() } else { top_folder }).or_default().add(r.size);
        if r.civitai_version_id.is_some() {
            linked += 1;
        }
        if let Some(m) = r.civitai_model_id {
            seen_models.insert(m);
        }
        if r.preview_path.is_some() {
            previews += 1;
        }
        if r.has_sidecar {
            sidecars += 1;
        }
        if r.damage.is_some() || r.sidecar_error.is_some() {
            damaged += 1;
        }
        if r.favorite {
            favorites += 1;
        }
        if let Some(c) = r.creator.as_deref().filter(|c| !c.is_empty()) {
            creators.entry(c.to_string()).or_default().add(r.size);
        }
        if let Some(sha) = r.sha256.as_deref().filter(|s| !s.is_empty()) {
            by_sha.entry(sha).or_default().push(r.size);
        }
        let age = at - r.modified;
        if age < 7.0 * 86400.0 {
            d7.add(r.size);
        }
        if age < 30.0 * 86400.0 {
            d30.add(r.size);
        }
        largest.push((r.size, r.id, r.model_name.as_str()));
    }
    let (mut dup_groups, mut dup_files, mut dup_bytes) = (0usize, 0usize, 0u64);
    for sizes in by_sha.values().filter(|v| v.len() > 1) {
        dup_groups += 1;
        dup_files += sizes.len();
        dup_bytes += sizes.iter().skip(1).sum::<u64>();
    }
    largest.sort_by(|a, b| b.0.cmp(&a.0));
    json!({
        "total": total.json(),
        "by_type": top(by_type, 10, false),
        "by_base": top(by_base, 40, false),
        "by_folder": top(by_folder, 20, true),
        "creators": top(creators, 15, false),
        "civitai_models": seen_models.len(),
        "linked": linked,
        "with_preview": previews,
        "with_sidecar": sidecars,
        "damaged": damaged,
        "favorites": favorites,
        "excluded": excluded,
        "duplicates": { "groups": dup_groups, "files": dup_files, "reclaimable": dup_bytes },
        "added_7d": d7.json(),
        "added_30d": d30.json(),
        "largest": largest.iter().take(10).map(|(s, id, n)| json!({ "id": id, "name": n, "size": s })).collect::<Vec<_>>(),
        "updates": updates,
    })
}

pub async fn stats(State(st): State<Arc<AppState>>, Query(q): Query<StatsQuery>) -> Json<Value> {
    let lib = st.library();
    let t = q.model_type.as_deref().and_then(ModelType::parse);
    let idx = st.updates.index(&lib);
    let updates = idx.by_model.values().filter(|id| lib.get(**id).is_some_and(|r| t.is_none_or(|t| t == r.model_type))).count();
    let lib2 = lib.clone();
    let at = now();
    let v = tokio::task::spawn_blocking(move || compute(&lib2, t, updates, at)).await.unwrap_or(Value::Null);
    Json(v)
}
