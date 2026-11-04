//! Civitai search / model view proxy and download queue endpoints.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use lmx_civitai::{
    client::CivitaiError,
    map::{self, Ownership},
    resort, SearchParams,
};
use lmx_core::{
    downloads::{DownloadJob, JobStatus},
    paths, ModelType,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::AppState;

fn err(status: StatusCode, code: &str, message: impl Into<String>) -> Response {
    (status, Json(json!({ "error": code, "message": message.into() }))).into_response()
}

fn civitai_err(e: CivitaiError) -> Response {
    match e {
        CivitaiError::NeedsKey => err(StatusCode::BAD_REQUEST, "needs_key", "Civitai の API キーが必要です"),
        CivitaiError::Http(404, _) => err(StatusCode::NOT_FOUND, "not_found", "Civitai に見つかりません"),
        CivitaiError::Http(401 | 403, m) if m.contains("権限") => err(StatusCode::FORBIDDEN, "forbidden", m),
        other => err(StatusCode::BAD_GATEWAY, "civitai", other.to_string()),
    }
}

fn split(s: &Option<String>) -> Vec<String> {
    s.as_deref()
        .map(|v| v.split(',').map(str::trim).filter(|x| !x.is_empty()).map(str::to_string).collect())
        .unwrap_or_default()
}

#[derive(Deserialize, Default)]
pub struct SearchQuery {
    pub q: Option<String>,
    /// lmx library type (lora/checkpoint/embedding/other) or "all".
    #[serde(rename = "type")]
    pub model_type: Option<String>,
    pub base: Option<String>,
    pub sort: Option<String>,
    pub period: Option<String>,
    pub nsfw: Option<u8>,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
    pub favorites: Option<u8>,
}

pub fn civitai_types(lmx: &str) -> Vec<String> {
    let v: &[&str] = match lmx {
        "lora" => &["LORA", "LoCon", "DoRA"],
        "checkpoint" => &["Checkpoint"],
        "embedding" => &["TextualInversion"],
        "other" => &["VAE", "Upscaler", "Controlnet", "MotionModule"],
        _ => &[],
    };
    v.iter().map(|s| s.to_string()).collect()
}

pub async fn search(State(st): State<Arc<AppState>>, Query(p): Query<SearchQuery>) -> Response {
    let params = SearchParams {
        query: p.q.clone().unwrap_or_default(),
        types: civitai_types(p.model_type.as_deref().unwrap_or("lora")),
        base_models: split(&p.base),
        sort: p.sort.clone().filter(|s| !s.is_empty()),
        period: p.period.clone().filter(|s| !s.is_empty()),
        nsfw: p.nsfw.unwrap_or(1) == 1,
        cursor: p.cursor.clone().filter(|s| !s.is_empty()),
        limit: p.limit.unwrap_or(40),
        favorites: p.favorites.unwrap_or(0) == 1,
        username: None,
    };
    let lib = st.library();
    let jobs = st.downloads.version_status();
    let own = Ownership { by_sha: Some(lib.sha_index()), by_version: Some(lib.version_index()), jobs: Some(&jobs) };

    // Civitai ignores `sort` when `query` is set: sort the relevance pool ourselves.
    if resort::wants_resort(&params.query, params.sort.as_deref()) {
        let (pool, capped) = match st.civitai.search_pool(&params, resort::POOL_MAX).await {
            Ok(v) => v,
            Err(e) => return civitai_err(e),
        };
        let mut sorted: Vec<Value> = pool.as_ref().clone();
        resort::sort_pool(&mut sorted, params.sort.as_deref().unwrap_or(""));
        let offset = resort::parse_cursor(params.cursor.as_deref());
        let limit = (params.limit.clamp(1, 100)) as usize;
        let items: Vec<Value> = sorted.iter().skip(offset).take(limit).map(|m| map::card(m, &own, &st.cfg.civitai_host)).collect();
        let next = (offset + limit < sorted.len()).then(|| format!("r{}", offset + limit));
        return Json(json!({
            "items": items,
            "next_cursor": next,
            "has_key": st.civitai.has_key(),
            "resorted": { "pool": sorted.len(), "capped": capped },
        }))
        .into_response();
    }

    let res = match st.civitai.search(&params).await {
        Ok(v) => v,
        Err(e) => return civitai_err(e),
    };
    let items: Vec<Value> = res
        .get("items")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(|m| map::card(m, &own, &st.cfg.civitai_host)).collect())
        .unwrap_or_default();
    let next = res.pointer("/metadata/nextCursor").cloned().unwrap_or(Value::Null);
    Json(json!({ "items": items, "next_cursor": next, "has_key": st.civitai.has_key(), "resorted": null })).into_response()
}

pub async fn model(State(st): State<Arc<AppState>>, Path(id): Path<i64>) -> Response {
    let m = match st.civitai.model(id).await {
        Ok(v) => v,
        Err(e) => return civitai_err(e),
    };
    let lib = st.library();
    let jobs = st.downloads.version_status();
    let own = Ownership { by_sha: Some(lib.sha_index()), by_version: Some(lib.version_index()), jobs: Some(&jobs) };
    Json(map::model_view(&m, &own, &st.cfg.civitai_host)).into_response()
}

#[derive(Deserialize)]
pub struct FavQuery {
    /// Comma separated model ids.
    pub ids: Option<String>,
}

/// Which of the given Civitai model ids are favorites of the API key's user (one request per 200).
pub async fn favorites(State(st): State<Arc<AppState>>, Query(p): Query<FavQuery>) -> Response {
    let ids: Vec<i64> = p.ids.as_deref().unwrap_or("").split(',').filter_map(|x| x.trim().parse().ok()).take(1000).collect();
    if ids.is_empty() {
        return Json(json!({ "favorite": [], "has_key": st.civitai.has_key() })).into_response();
    }
    match st.civitai.favorite_ids(&ids).await {
        Ok(f) => Json(json!({ "favorite": f, "has_key": true })).into_response(),
        Err(e) => civitai_err(e),
    }
}

#[derive(Deserialize)]
pub struct FavBody {
    pub on: bool,
}

/// Favorite / un-favorite a model on Civitai, then report the state Civitai actually has.
pub async fn set_favorite(State(st): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<FavBody>) -> Response {
    if let Err(e) = st.civitai.set_favorite(id, b.on).await {
        return civitai_err(e);
    }
    match st.civitai.favorite_ids(&[id]).await {
        Ok(f) => Json(json!({ "favorite": f.contains(&id) })).into_response(),
        Err(e) => civitai_err(e),
    }
}

#[derive(Deserialize)]
pub struct TargetsQuery {
    #[serde(rename = "type")]
    pub model_type: Option<String>,
}

/// Where a download of this type may go: configured roots and the folders already in use.
pub async fn targets(State(st): State<Arc<AppState>>, Query(p): Query<TargetsQuery>) -> Response {
    let t = p.model_type.as_deref().and_then(ModelType::parse).unwrap_or(ModelType::Lora);
    let roots: Vec<String> = st.cfg.roots.get(t).iter().map(|r| paths::norm(r)).collect();
    let folders: Vec<String> = st.library().folders(Some(t)).into_iter().map(|f| f.path).filter(|p| !p.is_empty()).collect();
    Json(json!({ "type": t, "roots": roots, "folders": folders })).into_response()
}

#[derive(Deserialize)]
pub struct EnqueueBody {
    pub model_id: i64,
    pub version_id: Option<i64>,
    pub file_id: Option<i64>,
    pub root: Option<String>,
    #[serde(default)]
    pub folder: String,
}

/// Build a job from a `/models/{id}` payload.
pub fn build_job(st: &AppState, model: &Value, version_id: Option<i64>, file_id: Option<i64>, root: Option<&str>, folder: &str) -> anyhow::Result<DownloadJob> {
    let version = match version_id {
        Some(v) => map::find_version(model, v).ok_or_else(|| anyhow::anyhow!("バージョン {v} が見つかりません"))?,
        None => model
            .get("modelVersions")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .ok_or_else(|| anyhow::anyhow!("このモデルにはバージョンがありません"))?,
    };
    let file = match file_id {
        Some(f) => version
            .get("files")
            .and_then(Value::as_array)
            .and_then(|fs| fs.iter().find(|x| x.get("id").and_then(Value::as_i64) == Some(f)))
            .ok_or_else(|| anyhow::anyhow!("ファイル {f} が見つかりません"))?,
        None => map::default_file(version).ok_or_else(|| anyhow::anyhow!("このバージョンにはファイルがありません"))?,
    };
    let ctype = model.get("type").and_then(Value::as_str).unwrap_or("");
    let t = map::lmx_type(ctype);
    let dir = st.cfg.download_dir(t, root, folder)?;
    let url = file
        .get("downloadUrl")
        .and_then(Value::as_str)
        .or_else(|| version.get("downloadUrl").and_then(Value::as_str))
        .ok_or_else(|| anyhow::anyhow!("ダウンロード URL がありません"))?;
    let image = version
        .get("images")
        .and_then(Value::as_array)
        .and_then(|imgs| imgs.iter().find(|i| i.get("url").and_then(Value::as_str).is_some()));
    // Keep the sidecar seed small: the model without its other versions, plus the chosen version.
    let mut model_slim = model.clone();
    if let Some(o) = model_slim.as_object_mut() {
        o.remove("modelVersions");
    }
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    Ok(DownloadJob {
        id: 0,
        created_at: 0.0,
        updated_at: 0.0,
        status: JobStatus::Queued,
        model_type: t,
        model_id: model.get("id").and_then(Value::as_i64).unwrap_or(0),
        version_id: version.get("id").and_then(Value::as_i64).unwrap_or(0),
        file_id: file.get("id").and_then(Value::as_i64),
        model_name: s(model, "name"),
        version_name: s(version, "name"),
        base_model: s(version, "baseModel"),
        file_name: paths::sanitize_file_name(&s(file, "name")),
        dest_dir: paths::norm(&dir),
        url: url.to_string(),
        size_bytes: (file.get("sizeKB").and_then(Value::as_f64).unwrap_or(0.0) * 1024.0) as u64,
        sha256: file.pointer("/hashes/SHA256").and_then(Value::as_str).map(str::to_ascii_lowercase),
        bytes_done: 0,
        error: None,
        // The first sample, video or not: video-model LoRAs often only have videos.
        image_url: image.and_then(|i| {
            let video = i.get("type").and_then(Value::as_str) == Some("video");
            i.get("url").and_then(Value::as_str).map(|u| map::preview_rendition_url(u, video))
        }),
        nsfw_level: image.and_then(|i| i.get("nsfwLevel").and_then(Value::as_i64)).unwrap_or(0),
        result_path: None,
        meta: Some(json!({ "model": model_slim, "version": version })),
    })
}

fn owned_already(st: &AppState, job: &DownloadJob) -> Option<i64> {
    let lib = st.library();
    job.sha256.as_ref().and_then(|s| lib.sha_index().get(s).copied())
}

pub async fn enqueue(State(st): State<Arc<AppState>>, Json(b): Json<EnqueueBody>) -> Response {
    let model = match st.civitai.model(b.model_id).await {
        Ok(v) => v,
        Err(e) => return civitai_err(e),
    };
    let job = match build_job(&st, &model, b.version_id, b.file_id, b.root.as_deref(), &b.folder) {
        Ok(j) => j,
        Err(e) => return err(StatusCode::BAD_REQUEST, "invalid", e.to_string()),
    };
    if let Some(local) = owned_already(&st, &job) {
        return err(StatusCode::CONFLICT, "owned", format!("既にライブラリにあります（id {local}）"));
    }
    match st.downloads.enqueue(job) {
        Ok(j) => (StatusCode::CREATED, Json(json!({ "job": public(&j) }))).into_response(),
        Err(e) => err(StatusCode::CONFLICT, "duplicate", e.to_string()),
    }
}

#[derive(Deserialize)]
pub struct UrlsBody {
    pub text: String,
    /// Target used for models of this lmx type; other types go to their first root.
    #[serde(rename = "type")]
    pub model_type: Option<String>,
    pub root: Option<String>,
    #[serde(default)]
    pub folder: String,
}

pub async fn enqueue_urls(State(st): State<Arc<AppState>>, Json(b): Json<UrlsBody>) -> Response {
    let wanted = b.model_type.as_deref().and_then(ModelType::parse);
    let mut results = Vec::new();
    for line in b.text.lines().map(str::trim).filter(|l| !l.is_empty()).take(50) {
        let outcome: Result<DownloadJob, String> = async {
            let r = lmx_civitai::url::parse(line).ok_or_else(|| "Civitai のモデル URL ではありません".to_string())?;
            let model_id = match (r.model_id, r.version_id) {
                (Some(m), _) => m,
                (None, Some(v)) => st
                    .civitai
                    .version(v)
                    .await
                    .map_err(|e| e.to_string())?
                    .get("modelId")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| "modelId がありません".to_string())?,
                (None, None) => return Err("ID がありません".into()),
            };
            let model = st.civitai.model(model_id).await.map_err(|e| e.to_string())?;
            let t = map::lmx_type(model.get("type").and_then(Value::as_str).unwrap_or(""));
            let (root, folder) = if Some(t) == wanted { (b.root.as_deref(), b.folder.as_str()) } else { (None, "") };
            let job = build_job(&st, &model, r.version_id, None, root, folder).map_err(|e| e.to_string())?;
            if let Some(local) = owned_already(&st, &job) {
                return Err(format!("既にライブラリにあります（id {local}）"));
            }
            st.downloads.enqueue(job).map_err(|e| e.to_string())
        }
        .await;
        results.push(match outcome {
            Ok(j) => json!({ "input": line, "ok": true, "job": public(&j) }),
            Err(e) => json!({ "input": line, "ok": false, "error": e }),
        });
    }
    Json(json!({ "results": results })).into_response()
}

fn public(j: &DownloadJob) -> Value {
    let mut v = serde_json::to_value(j).unwrap_or(Value::Null);
    if let Some(o) = v.as_object_mut() {
        o.remove("meta");
        o.remove("url");
    }
    v
}

pub async fn list(State(st): State<Arc<AppState>>) -> Json<Value> {
    let jobs: Vec<Value> = st.downloads.list().iter().rev().map(public).collect();
    Json(json!({ "jobs": jobs }))
}

pub async fn cancel(State(st): State<Arc<AppState>>, Path(id): Path<i64>) -> Response {
    match st.downloads.cancel_or_remove(id) {
        Ok(what) => Json(json!({ "result": what })).into_response(),
        Err(e) => err(StatusCode::NOT_FOUND, "not_found", e.to_string()),
    }
}

pub async fn retry(State(st): State<Arc<AppState>>, Path(id): Path<i64>) -> Response {
    match st.downloads.retry(id) {
        Ok(()) => Json(json!({ "result": "queued" })).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, "invalid", e.to_string()),
    }
}
