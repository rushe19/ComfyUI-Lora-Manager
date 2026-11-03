//! JSON API handlers.

use std::sync::{atomic::Ordering, Arc};

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use lmx_core::{library, paths, ModelRecord, ModelType, SortKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::AppState;

#[derive(Serialize)]
pub struct Health {
    pub status: &'static str,
    pub name: &'static str,
    pub version: &'static str,
}

pub async fn health() -> Json<Health> {
    Json(Health { status: "ok", name: "lmx", version: env!("CARGO_PKG_VERSION") })
}

fn not_found(what: &str) -> Response {
    (StatusCode::NOT_FOUND, Json(json!({ "error": format!("{what} not found") }))).into_response()
}

pub async fn status(State(st): State<Arc<AppState>>) -> Json<Value> {
    let lib = st.library();
    let counts: serde_json::Map<String, Value> =
        lib.counts().into_iter().map(|(t, n)| (t.as_str().to_string(), json!(n))).collect();
    let roots: serde_json::Map<String, Value> = ModelType::ALL
        .iter()
        .map(|t| (t.as_str().to_string(), json!(st.cfg.roots.get(*t).iter().map(|p| paths::norm(p)).collect::<Vec<_>>())))
        .collect();
    Json(json!({
        "total": lib.len(),
        "counts": counts,
        "scanning": st.scanning.load(Ordering::SeqCst),
        "progress": {
            "walked_files": st.progress.walked_files.load(Ordering::Relaxed),
            "to_parse": st.progress.to_parse.load(Ordering::Relaxed),
            "parsed": st.progress.parsed.load(Ordering::Relaxed),
        },
        "last_scan": *st.last_scan.lock().expect("last_scan lock"),
        "roots": roots,
        "civitai_host": st.cfg.civitai_host,
    }))
}

pub async fn start_scan(State(st): State<Arc<AppState>>) -> Response {
    let started = st.spawn_scan();
    (StatusCode::ACCEPTED, Json(json!({ "started": started }))).into_response()
}

#[derive(Debug, Deserialize, Default)]
pub struct ListParams {
    #[serde(rename = "type")]
    pub model_type: Option<String>,
    pub q: Option<String>,
    /// Comma-separated base models.
    pub base: Option<String>,
    pub tag: Option<String>,
    pub folder: Option<String>,
    pub recursive: Option<u8>,
    pub fav: Option<u8>,
    pub damaged: Option<u8>,
    pub sort: Option<String>,
    pub order: Option<String>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

impl ListParams {
    pub fn to_query(&self) -> library::Query {
        library::Query {
            model_type: self.model_type.as_deref().and_then(ModelType::parse),
            text: self.q.clone().unwrap_or_default(),
            base_models: self
                .base
                .as_deref()
                .map(|b| b.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect())
                .unwrap_or_default(),
            tag: self.tag.clone().filter(|t| !t.is_empty()),
            folder: self.folder.clone(),
            recursive: self.recursive.unwrap_or(1) == 1,
            favorites_only: self.fav.unwrap_or(0) == 1,
            damaged_only: self.damaged.unwrap_or(0) == 1,
            include_excluded: false,
            sort: SortKey::parse(self.sort.as_deref().unwrap_or("name")),
            descending: self.order.as_deref() == Some("desc"),
            offset: self.offset.unwrap_or(0),
            limit: self.limit.unwrap_or(100),
        }
    }
}

/// Compact card data for the grid.
#[derive(Serialize)]
pub struct Summary<'a> {
    pub id: i64,
    #[serde(rename = "type")]
    pub model_type: ModelType,
    pub name: &'a str,
    pub file_name: &'a str,
    pub folder: &'a str,
    pub base_model: &'a str,
    pub size: u64,
    pub modified: f64,
    pub favorite: bool,
    pub nsfw_level: i64,
    pub has_metadata: bool,
    pub damaged: bool,
    pub preview: Option<String>,
    pub preview_kind: Option<&'static str>,
}

pub fn preview_url(r: &ModelRecord) -> Option<String> {
    r.preview_path.as_ref().map(|_| {
        let v = r.sidecar_mtime.unwrap_or(r.modified) as i64;
        format!("/api/preview/{}?v={v}", r.id)
    })
}

pub fn summary(r: &ModelRecord) -> Summary<'_> {
    Summary {
        id: r.id,
        model_type: r.model_type,
        name: &r.model_name,
        file_name: &r.file_name,
        folder: &r.folder,
        base_model: &r.base_model,
        size: r.size,
        modified: r.modified,
        favorite: r.favorite,
        nsfw_level: r.preview_nsfw_level,
        has_metadata: r.has_sidecar,
        damaged: r.damage.is_some() || r.sidecar_error.is_some(),
        preview: preview_url(r),
        preview_kind: r.preview_path.as_deref().map(|p| if paths::is_video(p) { "video" } else { "image" }),
    }
}

pub async fn list_models(State(st): State<Arc<AppState>>, Query(p): Query<ListParams>) -> Json<Value> {
    let lib = st.library();
    let q = p.to_query();
    let (page, total) = lib.query(&q);
    let items: Vec<Summary> = page.iter().map(|r| summary(r)).collect();
    let next = q.offset + items.len();
    Json(json!({
        "total": total,
        "offset": q.offset,
        "next_offset": if next < total { Some(next) } else { None },
        "items": items,
    }))
}

fn civitai_images(c: &serde_json::Map<String, Value>) -> Vec<Value> {
    c.get("images")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .take(24)
                .filter_map(|img| {
                    let url = img.get("url")?.as_str()?;
                    Some(json!({
                        "url": url,
                        "nsfw_level": img.get("nsfwLevel").cloned().unwrap_or(Value::Null),
                        "type": img.get("type").cloned().unwrap_or(json!("image")),
                        "width": img.get("width").cloned().unwrap_or(Value::Null),
                        "height": img.get("height").cloned().unwrap_or(Value::Null),
                    }))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Strength from LoRA Manager's `usage_tips` JSON string (e.g. `{"strength": 0.8}`), default 1.0.
pub fn strength_from_usage_tips(tips: Option<&str>) -> f64 {
    tips.and_then(|t| serde_json::from_str::<Value>(t).ok())
        .and_then(|v| v.get("strength").and_then(Value::as_f64))
        .unwrap_or(1.0)
}

pub async fn model_detail(State(st): State<Arc<AppState>>, Path(id): Path<i64>) -> Response {
    let Some(r) = st.library().get(id) else { return not_found("model") };
    // The sidecar is the source of truth for long fields; read it fresh (read-only).
    let side = tokio::task::spawn_blocking({
        let path = paths::sidecar_path(std::path::Path::new(&r.file_path));
        move || lmx_core::sidecar::read(&path)
    })
    .await
    .ok()
    .and_then(|r| r.ok())
    .flatten();
    let data = side.as_ref().map(|s| s.data.clone()).unwrap_or_default();
    let civ = data.get("civitai").and_then(Value::as_object).cloned().unwrap_or_default();
    let get_str = |m: &serde_json::Map<String, Value>, k: &str| m.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let usage_tips = data.get("usage_tips").and_then(Value::as_str).map(str::to_string);
    let strength = strength_from_usage_tips(usage_tips.as_deref());
    let civitai_url = r.civitai_model_id.map(|m| {
        let mut u = format!("https://{}/models/{m}", st.cfg.civitai_host);
        if let Some(v) = r.civitai_version_id {
            u.push_str(&format!("?modelVersionId={v}"));
        }
        u
    });
    let lora_syntax = (r.model_type == ModelType::Lora).then(|| format!("<lora:{}:{}>", r.file_name, fmt_strength(strength)));
    Json(json!({
        "summary": summary(&r),
        "file_path": r.file_path,
        "root": r.root,
        "sha256": r.sha256,
        "tags": r.tags,
        "trained_words": r.trained_words,
        "creator": r.creator,
        "civitai_model_id": r.civitai_model_id,
        "civitai_version_id": r.civitai_version_id,
        "civitai_url": civitai_url,
        "version_name": get_str(&civ, "name"),
        "metadata_source": r.metadata_source,
        "damage": r.damage,
        "sidecar_error": r.sidecar_error,
        "model_description": get_str(&data, "modelDescription"),
        "version_description": get_str(&civ, "description"),
        "notes": get_str(&data, "notes"),
        "usage_tips": usage_tips,
        "strength": strength,
        "lora_syntax": lora_syntax,
        "images": civitai_images(&civ),
    }))
    .into_response()
}

fn fmt_strength(s: f64) -> String {
    let t = format!("{s:.2}");
    let t = t.trim_end_matches('0').trim_end_matches('.');
    if t.contains('.') { t.to_string() } else { format!("{t}.0") }
}

#[derive(Deserialize)]
pub struct TypeParam {
    #[serde(rename = "type")]
    pub model_type: Option<String>,
}

pub async fn folders(State(st): State<Arc<AppState>>, Query(p): Query<TypeParam>) -> Json<Value> {
    let t = p.model_type.as_deref().and_then(ModelType::parse);
    Json(json!(st.library().folders(t)))
}

pub async fn facets(State(st): State<Arc<AppState>>, Query(p): Query<TypeParam>) -> Json<Value> {
    let t = p.model_type.as_deref().and_then(ModelType::parse);
    let lib = st.library();
    Json(json!({ "base_models": lib.base_models(t), "tags": lib.tags(t, 200) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strength_parsing_and_format() {
        assert_eq!(strength_from_usage_tips(Some(r#"{"strength": 0.8}"#)), 0.8);
        assert_eq!(strength_from_usage_tips(Some("{}")), 1.0);
        assert_eq!(strength_from_usage_tips(Some("not json")), 1.0);
        assert_eq!(fmt_strength(1.0), "1.0");
        assert_eq!(fmt_strength(0.8), "0.8");
        assert_eq!(fmt_strength(0.75), "0.75");
    }
}
