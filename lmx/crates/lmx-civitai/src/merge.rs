//! Merge Civitai-shaped version metadata into an existing sidecar, following LoRA Manager's
//! `MetadataSyncService.update_model_metadata` + `fetch_and_update_model` rules.

use serde_json::{json, Map, Value};

use crate::map::civitai_block;

/// LoRA Manager `BASE_MODEL_MAPPING` (substring match on the lowercase name, first hit wins;
/// the "Hunyuan Video" key never matches because it is compared against lowercase input).
const BASE_MODEL_MAPPING: &[(&str, &str)] = &[
    ("sd_1.5", "SD 1.5"),
    ("sd-v1-5", "SD 1.5"),
    ("sd-v2-1", "SD 2.1"),
    ("sdxl", "SDXL 1.0"),
    ("sd-v2", "SD 2.0"),
    ("flux1", "Flux.1 D"),
    ("flux.1 d", "Flux.1 D"),
    ("illustrious", "Illustrious"),
    ("il", "Illustrious"),
    ("pony", "Pony"),
    ("Hunyuan Video", "Hunyuan Video"),
];

pub fn determine_base_model(v: Option<&str>) -> String {
    let Some(s) = v.filter(|s| !s.is_empty()) else { return "Unknown".into() };
    let lower = s.to_lowercase();
    for (k, val) in BASE_MODEL_MAPPING {
        if lower.contains(k) {
            return (*val).into();
        }
    }
    s.to_string()
}

/// "Full" Civitai API payload: has files and images and does not come from an archive.
pub fn is_api_quality(civ: &Value) -> bool {
    let nonempty = |k: &str| civ.get(k).and_then(Value::as_array).is_some_and(|a| !a.is_empty());
    let source = civ.get("source").and_then(Value::as_str).unwrap_or("");
    nonempty("files") && nonempty("images") && source != "archive_db" && source != "civarchive"
}

fn merge_unique(a: &[Value], b: &[Value]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::with_capacity(a.len() + b.len());
    for v in a.iter().chain(b) {
        if !out.contains(v) {
            out.push(v.clone());
        }
    }
    out
}

fn is_blank(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => true,
        Some(Value::String(s)) => s.is_empty(),
        Some(Value::Array(a)) => a.is_empty(),
        _ => false,
    }
}

/// LoRA Manager-shaped payload for a Civitai API hit: `/model-versions/by-hash` enriched with the
/// `/models` object (description, tags, creator, normalized license), ComfyUI meta removed.
pub fn api_payload(version: &Value, model: &Value) -> Value {
    Value::Object(civitai_block(version, model))
}

/// Archive payload as LoRA Manager builds it, but with the real license fields from the archived
/// model instead of LoRA Manager's defaults (its archive path drops them).
pub fn archive_payload(version: &Value, model_data: &Value) -> Value {
    let mut v = version.clone();
    if let Some(m) = v.get_mut("model").and_then(Value::as_object_mut) {
        for (k, val) in crate::map::resolve_license_payload(model_data) {
            m.insert(k, val);
        }
    }
    v
}

/// Apply remote metadata (`incoming`, LoRA Manager-shaped) to a sidecar map, following
/// `MetadataSyncService.update_model_metadata`. `source` is recorded as `metadata_source`;
/// `db_checked` says whether the archive was consulted.
pub fn merge_remote(local: &mut Map<String, Value>, incoming: &Value, source: &str, db_checked: bool, now: f64) {
    local.insert("from_civitai".into(), json!(true));
    local.insert("db_checked".into(), json!(db_checked));
    local.insert("metadata_source".into(), json!(source));
    local.insert("last_checked_at".into(), json!(now));

    let existing = local.get("civitai").cloned().filter(Value::is_object).unwrap_or_else(|| json!({}));
    if !is_api_quality(incoming) && is_api_quality(&existing) {
        // Keep the richer payload already on disk.
    } else {
        let mut merged = existing.as_object().cloned().unwrap_or_default();
        if let Some(inc) = incoming.as_object() {
            for (k, v) in inc {
                merged.insert(k.clone(), v.clone());
            }
        }
        if let Some(old) = existing.get("trainedWords") {
            let old = old.as_array().cloned().unwrap_or_default();
            let new = incoming.get("trainedWords").and_then(Value::as_array).cloned().unwrap_or_default();
            merged.insert("trainedWords".into(), Value::Array(merge_unique(&old, &new)));
        }
        local.insert("civitai".into(), Value::Object(merged));
    }

    if let Some(m) = incoming.get("model").filter(|m| m.as_object().is_some_and(|o| !o.is_empty())) {
        if let Some(name) = m.get("name").filter(|n| !is_blank(Some(n))) {
            local.insert("model_name".into(), name.clone());
        }
        if is_blank(local.get("modelDescription")) && !is_blank(m.get("description")) {
            local.insert("modelDescription".into(), m["description"].clone());
        }
        if is_blank(local.get("tags")) && !is_blank(m.get("tags")) {
            local.insert("tags".into(), m["tags"].clone());
        }
        let civ = local.entry("civitai").or_insert_with(|| json!({}));
        if let Some(c) = civ.as_object_mut() {
            if !is_blank(m.get("creator")) && is_blank(c.get("creator")) {
                c.insert("creator".into(), m["creator"].clone());
            }
            let mut cm = c.get("model").and_then(Value::as_object).cloned().unwrap_or_default();
            for (k, v) in crate::map::resolve_license_payload(m) {
                cm.insert(k, v);
            }
            c.insert("model".into(), Value::Object(cm));
        }
    }

    local.insert("base_model".into(), json!(determine_base_model(incoming.get("baseModel").and_then(Value::as_str))));

    // AutoV3 of the file whose sha256 matches ours.
    let sha = local.get("sha256").and_then(Value::as_str).unwrap_or("").to_ascii_lowercase();
    if !sha.is_empty() {
        let files = local.get("civitai").and_then(|c| c.get("files")).and_then(Value::as_array).cloned().unwrap_or_default();
        let auto = files.iter().find_map(|f| {
            let h = f.pointer("/hashes/SHA256").and_then(Value::as_str)?;
            if h.eq_ignore_ascii_case(&sha) {
                f.pointer("/hashes/AutoV3").and_then(Value::as_str).map(str::to_ascii_lowercase)
            } else {
                None
            }
        });
        if let Some(a) = auto {
            local.insert("autov3".into(), json!(a));
        }
    }
}

/// First still image of a version payload (url, nsfwLevel), for a missing preview.
pub fn first_image(version: &Value) -> Option<(String, i64)> {
    version.get("images")?.as_array()?.iter().find_map(|i| {
        if i.get("type").and_then(Value::as_str) == Some("video") {
            return None;
        }
        let url = i.get("url").and_then(Value::as_str)?;
        Some((url.to_string(), i.get("nsfwLevel").and_then(Value::as_i64).unwrap_or(0)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    #[test]
    fn base_model_mapping_matches_lora_manager() {
        assert_eq!(determine_base_model(Some("Illustrious")), "Illustrious");
        assert_eq!(determine_base_model(Some("SDXL Hyper")), "SDXL 1.0");
        assert_eq!(determine_base_model(Some("Pony")), "Pony");
        assert_eq!(determine_base_model(Some("NoobAI")), "NoobAI");
        assert_eq!(determine_base_model(Some("SD 1.5")), "SD 1.5");
        assert_eq!(determine_base_model(Some("Hunyuan Video")), "Hunyuan Video");
        assert_eq!(determine_base_model(None), "Unknown");
    }

    #[test]
    fn merge_keeps_user_fields_unknown_keys_and_order() {
        let version = fixture("version.json");
        let mut local: Map<String, Value> = serde_json::from_value(json!({
            "file_name": "x", "model_name": "x", "sha256": "abc", "base_model": "Unknown",
            "notes": "my note", "from_civitai": false, "civitai": {"trainedWords": ["mine", "frieren"]},
            "tags": ["keep"], "modelDescription": "", "favorite": true, "custom_key": 42,
            "metadata_source": null, "last_checked_at": 0
        }))
        .unwrap();
        merge_remote(&mut local, &version, "archive_db", true, 9.0);
        let keys: Vec<&str> = local.keys().map(String::as_str).collect();
        assert_eq!(&keys[..12], ["file_name", "model_name", "sha256", "base_model", "notes", "from_civitai", "civitai", "tags", "modelDescription", "favorite", "custom_key", "metadata_source"]);
        assert_eq!(local["notes"], "my note");
        assert_eq!(local["favorite"], true);
        assert_eq!(local["custom_key"], 42);
        assert_eq!(local["tags"], json!(["keep"]));
        assert_eq!(local["model_name"], version["model"]["name"]);
        assert_eq!(local["base_model"], "Illustrious");
        assert_eq!(local["from_civitai"], true);
        assert_eq!(local["metadata_source"], "archive_db");
        assert_eq!(local["db_checked"], true);
        assert_eq!(local["civitai"]["id"], version["id"]);
        let words = local["civitai"]["trainedWords"].as_array().unwrap();
        assert_eq!(words[0], "mine", "saved order first");
        assert_eq!(words.iter().filter(|w| *w == "frieren").count(), 1);
    }

    #[test]
    fn archive_payload_does_not_replace_full_api_payload() {
        let version = fixture("version.json");
        let mut local: Map<String, Value> = serde_json::from_value(json!({"civitai": version.clone(), "sha256": "x"})).unwrap();
        let mut archive = version.clone();
        archive["source"] = json!("archive_db");
        archive["name"] = json!("archived name");
        merge_remote(&mut local, &archive, "archive_db", true, 1.0);
        assert_eq!(local["civitai"]["name"], version["name"]);
    }
}
