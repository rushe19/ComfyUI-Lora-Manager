//! Pure transforms from Civitai JSON to what the PWA needs, annotated with local ownership.

use std::{collections::HashMap, sync::OnceLock};

use lmx_core::ModelType;
use regex::Regex;
use serde_json::{json, Map, Value};

/// What the library already has / what is queued, for "所持済み" badges.
#[derive(Default)]
pub struct Ownership<'a> {
    /// lowercase sha256 -> local model id
    pub by_sha: Option<&'a HashMap<String, i64>>,
    /// civitai version id -> local model id
    pub by_version: Option<&'a HashMap<i64, i64>>,
    /// civitai version id -> download job status
    pub jobs: Option<&'a HashMap<i64, String>>,
}

impl Ownership<'_> {
    fn local_for_file(&self, f: &Value) -> Option<i64> {
        let sha = f.pointer("/hashes/SHA256").and_then(Value::as_str)?.to_ascii_lowercase();
        self.by_sha?.get(&sha).copied()
    }

    /// Local model id owning this version (by version id or by any file hash).
    pub fn local_for_version(&self, v: &Value) -> Option<i64> {
        let vid = v.get("id").and_then(Value::as_i64);
        if let (Some(map), Some(vid)) = (self.by_version, vid) {
            if let Some(id) = map.get(&vid) {
                return Some(*id);
            }
        }
        v.get("files").and_then(Value::as_array)?.iter().find_map(|f| self.local_for_file(f))
    }

    fn job(&self, v: &Value) -> Option<String> {
        let vid = v.get("id").and_then(Value::as_i64)?;
        self.jobs?.get(&vid).cloned()
    }
}

/// Map a Civitai model type to the lmx library it belongs to.
pub fn lmx_type(civitai_type: &str) -> ModelType {
    match civitai_type.to_ascii_lowercase().as_str() {
        "lora" | "locon" | "dora" | "lycoris" => ModelType::Lora,
        "checkpoint" => ModelType::Checkpoint,
        "textualinversion" => ModelType::Embedding,
        _ => ModelType::Other,
    }
}

/// Ask the image CDN for a smaller rendition (`/original=true/` or `/width=N/` -> `/width=W/`).
pub fn resize_image_url(url: &str, width: u32) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"/(original=true|width=\d+)[^/]*/").expect("regex"));
    if re.is_match(url) {
        re.replace(url, format!("/width={width}/").as_str()).into_owned()
    } else {
        url.to_string()
    }
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn image_view(img: &Value, width: u32) -> Value {
    let url = s(img, "url");
    let kind = s(img, "type");
    json!({
        "url": if kind == "video" { url.to_string() } else { resize_image_url(url, width) },
        "full": url,
        "type": if kind.is_empty() { "image" } else { kind },
        "nsfw_level": img.get("nsfwLevel").and_then(Value::as_i64).unwrap_or(0),
        "width": img.get("width").cloned().unwrap_or(Value::Null),
        "height": img.get("height").cloned().unwrap_or(Value::Null),
    })
}

fn versions(model: &Value) -> &[Value] {
    model.get("modelVersions").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

/// First still image of the newest version (videos only if nothing else exists).
fn cover(model: &Value) -> Option<&Value> {
    let imgs: Vec<&Value> = versions(model)
        .iter()
        .flat_map(|v| v.get("images").and_then(Value::as_array).map(|a| a.iter().collect::<Vec<_>>()).unwrap_or_default())
        .collect();
    imgs.iter().find(|i| s(i, "type") != "video").or(imgs.first()).copied()
}

/// Compact card for search / favorites grids.
pub fn card(model: &Value, own: &Ownership, host: &str) -> Value {
    let vs = versions(model);
    let owned = vs.iter().filter(|v| own.local_for_version(v).is_some()).count();
    let queued = vs.iter().any(|v| own.job(v).is_some_and(|j| j == "queued" || j == "running"));
    let mut bases: Vec<&str> = Vec::new();
    for v in vs {
        let b = s(v, "baseModel");
        if !b.is_empty() && !bases.contains(&b) {
            bases.push(b);
        }
    }
    let id = model.get("id").and_then(Value::as_i64).unwrap_or(0);
    let ctype = s(model, "type");
    json!({
        "id": id,
        "name": s(model, "name"),
        "type": ctype,
        "lmx_type": lmx_type(ctype),
        "nsfw": model.get("nsfw").and_then(Value::as_bool).unwrap_or(false),
        "nsfw_level": model.get("nsfwLevel").and_then(Value::as_i64).unwrap_or(0),
        "creator": model.pointer("/creator/username").and_then(Value::as_str),
        "tags": model.get("tags").and_then(Value::as_array).map(|t| t.iter().take(6).cloned().collect::<Vec<_>>()).unwrap_or_default(),
        "downloads": model.pointer("/stats/downloadCount").and_then(Value::as_i64).unwrap_or(0),
        "thumbs_up": model.pointer("/stats/thumbsUpCount").and_then(Value::as_i64).unwrap_or(0),
        "base_models": bases.into_iter().take(3).collect::<Vec<_>>(),
        "versions": vs.len(),
        "owned_versions": owned,
        "queued": queued,
        "image": cover(model).map(|i| image_view(i, 450)),
        "url": format!("https://{host}/models/{id}"),
    })
}

/// Full model view with per-version and per-file ownership.
pub fn model_view(model: &Value, own: &Ownership, host: &str) -> Value {
    let id = model.get("id").and_then(Value::as_i64).unwrap_or(0);
    let ctype = s(model, "type");
    let versions: Vec<Value> = versions(model)
        .iter()
        .map(|v| {
            let files: Vec<Value> = v
                .get("files")
                .and_then(Value::as_array)
                .map(|fs| {
                    fs.iter()
                        .map(|f| {
                            json!({
                                "id": f.get("id").and_then(Value::as_i64),
                                "name": s(f, "name"),
                                "size_kb": f.get("sizeKB").and_then(Value::as_f64).unwrap_or(0.0),
                                "type": s(f, "type"),
                                "fp": f.pointer("/metadata/fp").and_then(Value::as_str),
                                "format": f.pointer("/metadata/format").and_then(Value::as_str),
                                "primary": f.get("primary").and_then(Value::as_bool).unwrap_or(false),
                                "sha256": f.pointer("/hashes/SHA256").and_then(Value::as_str).map(str::to_ascii_lowercase),
                                "owned_local_id": own.local_for_file(f),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            json!({
                "id": v.get("id").and_then(Value::as_i64),
                "name": s(v, "name"),
                "base_model": s(v, "baseModel"),
                "published_at": v.get("publishedAt").cloned().unwrap_or(Value::Null),
                "availability": v.get("availability").cloned().unwrap_or(Value::Null),
                "trained_words": v.get("trainedWords").cloned().unwrap_or(json!([])),
                "downloads": v.pointer("/stats/downloadCount").and_then(Value::as_i64).unwrap_or(0),
                "owned_local_id": own.local_for_version(v),
                "job": own.job(v),
                "description": s(v, "description"),
                "images": v.get("images").and_then(Value::as_array).map(|a| a.iter().take(12).map(|i| image_view(i, 450)).collect::<Vec<_>>()).unwrap_or_default(),
                "files": files,
            })
        })
        .collect();
    json!({
        "id": id,
        "name": s(model, "name"),
        "type": ctype,
        "lmx_type": lmx_type(ctype),
        "nsfw": model.get("nsfw").and_then(Value::as_bool).unwrap_or(false),
        "creator": model.pointer("/creator/username").and_then(Value::as_str),
        "tags": model.get("tags").cloned().unwrap_or(json!([])),
        "description": s(model, "description"),
        "url": format!("https://{host}/models/{id}"),
        "versions": versions,
    })
}

/// The file to download when the user does not pick one: primary, else the first "Model" file.
pub fn default_file(version: &Value) -> Option<&Value> {
    let files = version.get("files").and_then(Value::as_array)?;
    files
        .iter()
        .find(|f| f.get("primary").and_then(Value::as_bool).unwrap_or(false))
        .or_else(|| files.iter().find(|f| s(f, "type") == "Model"))
        .or(files.first())
}

/// Find a version inside a `/models/{id}` payload.
pub fn find_version(model: &Value, version_id: i64) -> Option<&Value> {
    versions(model).iter().find(|v| v.get("id").and_then(Value::as_i64) == Some(version_id))
}

/// Merge model-level fields into a `/model-versions/{id}` payload the way LoRA Manager stores it.
pub fn civitai_block(version: &Value, model: &Value) -> Map<String, Value> {
    let mut civ = version.as_object().cloned().unwrap_or_default();
    if !civ.contains_key("modelId") {
        if let Some(id) = model.get("id") {
            civ.insert("modelId".into(), id.clone());
        }
    }
    let mut m = civ.get("model").and_then(Value::as_object).cloned().unwrap_or_default();
    for k in ["name", "type", "nsfw", "poi"] {
        if !m.contains_key(k) {
            if let Some(v) = model.get(k) {
                m.insert(k.into(), v.clone());
            }
        }
    }
    for k in ["description", "tags", "allowNoCredit", "allowCommercialUse", "allowDerivatives", "allowDifferentLicense"] {
        if let Some(v) = model.get(k) {
            m.insert(k.into(), v.clone());
        }
    }
    civ.insert("model".into(), Value::Object(m));
    if let Some(c) = model.get("creator") {
        civ.insert("creator".into(), c.clone());
    }
    civ
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    #[test]
    fn resize_rewrites_cdn_segment() {
        let u = "https://image.civitai.com/key/uuid/original=true/123.jpeg";
        assert_eq!(resize_image_url(u, 450), "https://image.civitai.com/key/uuid/width=450/123.jpeg");
        let w = "https://image.civitai.com/key/uuid/width=1024/123.jpeg";
        assert_eq!(resize_image_url(w, 450), "https://image.civitai.com/key/uuid/width=450/123.jpeg");
        assert_eq!(resize_image_url("https://x/y.png", 450), "https://x/y.png");
    }

    #[test]
    fn types_map_to_libraries() {
        assert_eq!(lmx_type("LORA"), ModelType::Lora);
        assert_eq!(lmx_type("LoCon"), ModelType::Lora);
        assert_eq!(lmx_type("Checkpoint"), ModelType::Checkpoint);
        assert_eq!(lmx_type("TextualInversion"), ModelType::Embedding);
        assert_eq!(lmx_type("VAE"), ModelType::Other);
    }

    #[test]
    fn cards_from_real_search_response_mark_owned_by_hash() {
        let search = fixture("search_frieren.json");
        let items = search["items"].as_array().unwrap();
        let first = &items[0];
        let sha = first["modelVersions"][0]["files"][0]["hashes"]["SHA256"].as_str().unwrap().to_ascii_lowercase();
        let by_sha: HashMap<String, i64> = [(sha, 42)].into();
        let own = Ownership { by_sha: Some(&by_sha), ..Default::default() };
        let c = card(first, &own, "civitai.red");
        assert_eq!(c["owned_versions"], 1);
        assert_eq!(c["lmx_type"], "lora");
        assert!(c["image"]["url"].as_str().unwrap().contains("/width=450/"));
        assert!(c["url"].as_str().unwrap().starts_with("https://civitai.red/models/"));
        let other = card(&items[1], &own, "civitai.red");
        assert_eq!(other["owned_versions"], 0);
    }

    #[test]
    fn model_view_lists_versions_files_and_jobs() {
        let model = fixture("model.json");
        let vid = model["modelVersions"][0]["id"].as_i64().unwrap();
        let by_version: HashMap<i64, i64> = [(vid, 7)].into();
        let jobs: HashMap<i64, String> = [(vid, "queued".to_string())].into();
        let own = Ownership { by_version: Some(&by_version), jobs: Some(&jobs), ..Default::default() };
        let v = model_view(&model, &own, "civitai.com");
        let v0 = &v["versions"][0];
        assert_eq!(v0["owned_local_id"], 7);
        assert_eq!(v0["job"], "queued");
        assert!(v0["files"].as_array().unwrap().iter().any(|f| f["type"] == "Model" && f["sha256"].as_str().is_some()));
        assert!(default_file(&model["modelVersions"][0]).is_some());
        assert_eq!(find_version(&model, vid).unwrap()["id"], vid);
    }

    #[test]
    fn civitai_block_matches_lora_manager_shape() {
        let model = fixture("model.json");
        let version = fixture("version.json");
        let civ = civitai_block(&version, &model);
        assert_eq!(civ["id"], version["id"]);
        assert_eq!(civ["model"]["name"], model["name"]);
        assert_eq!(civ["model"]["tags"], model["tags"]);
        assert!(civ["model"].get("description").is_some());
        assert_eq!(civ["creator"], model["creator"]);
        assert!(civ.get("files").is_some() && civ.get("images").is_some());
        // creator is appended last, like LoRA Manager's sidecars
        assert_eq!(civ.keys().last().unwrap(), "creator");
    }
}
