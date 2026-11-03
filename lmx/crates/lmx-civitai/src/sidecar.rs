//! Build a LoRA Manager compatible `<stem>.metadata.json` for a freshly downloaded file.

use serde_json::{json, Map, Value};

use crate::map::civitai_block;

pub struct LocalFacts<'a> {
    pub file_stem: &'a str,
    pub model_path: &'a str,
    pub size: u64,
    pub modified: f64,
    pub sha256: &'a str,
    pub preview_path: Option<&'a str>,
    pub preview_nsfw_level: i64,
    pub now: f64,
}

/// Key order follows sidecars written by LoRA Manager (see tests/fixtures in lmx-core).
pub fn build(version: &Value, model: &Value, file: &Value, local: &LocalFacts) -> Map<String, Value> {
    let civ = civitai_block(version, model);
    let mut m = Map::new();
    m.insert("file_name".into(), json!(local.file_stem));
    m.insert("model_name".into(), model.get("name").cloned().unwrap_or(json!(local.file_stem)));
    m.insert("file_path".into(), json!(local.model_path));
    m.insert("size".into(), json!(local.size));
    m.insert("modified".into(), json!(local.modified));
    m.insert("sha256".into(), json!(local.sha256.to_ascii_lowercase()));
    m.insert("base_model".into(), version.get("baseModel").cloned().unwrap_or(json!("Unknown")));
    m.insert("preview_url".into(), json!(local.preview_path.unwrap_or("")));
    m.insert("preview_nsfw_level".into(), json!(local.preview_nsfw_level));
    m.insert("notes".into(), json!(""));
    m.insert("from_civitai".into(), json!(true));
    m.insert("civitai".into(), Value::Object(civ));
    m.insert("tags".into(), model.get("tags").cloned().unwrap_or(json!([])));
    m.insert("modelDescription".into(), model.get("description").cloned().unwrap_or(json!("")));
    m.insert("civitai_deleted".into(), json!(false));
    m.insert("favorite".into(), json!(false));
    m.insert("exclude".into(), json!(false));
    m.insert("db_checked".into(), json!(false));
    m.insert("skip_metadata_refresh".into(), json!(false));
    m.insert("metadata_source".into(), json!("civitai_api"));
    m.insert("last_checked_at".into(), json!(local.now));
    m.insert("hash_status".into(), json!("completed"));
    m.insert(
        "autov3".into(),
        file.pointer("/hashes/AutoV3").and_then(Value::as_str).map(|s| json!(s.to_ascii_lowercase())).unwrap_or(Value::Null),
    );
    m.insert("quantization".into(), Value::Null);
    m.insert("usage_tips".into(), json!("{}"));
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    #[test]
    fn sidecar_is_readable_by_lmx_core_and_has_lm_keys() {
        let model = fixture("model.json");
        let version = fixture("version.json");
        let file = crate::map::default_file(&version).unwrap().clone();
        let local = LocalFacts {
            file_stem: "frieren",
            model_path: "H:/m/Lora/Character/frieren.safetensors",
            size: 123,
            modified: 1.5,
            sha256: "ABCDEF",
            preview_path: Some("H:/m/Lora/Character/frieren.preview.jpeg"),
            preview_nsfw_level: 1,
            now: 2.5,
        };
        let m = build(&version, &model, &file, &local);
        let expected = [
            "file_name", "model_name", "file_path", "size", "modified", "sha256", "base_model", "preview_url", "preview_nsfw_level",
            "notes", "from_civitai", "civitai", "tags", "modelDescription", "civitai_deleted", "favorite", "exclude", "db_checked",
            "skip_metadata_refresh", "metadata_source", "last_checked_at", "hash_status", "autov3", "quantization", "usage_tips",
        ];
        assert_eq!(m.keys().map(String::as_str).collect::<Vec<_>>(), expected);
        assert_eq!(m["sha256"], "abcdef");
        assert_eq!(m["base_model"], version["baseModel"]);
        // Round-trip through lmx-core's tolerant reader and record builder.
        let bytes = serde_json::to_vec_pretty(&Value::Object(m)).unwrap();
        let read = lmx_core::sidecar::parse(&bytes).unwrap();
        assert!(read.damage.is_none());
        assert_eq!(read.data["civitai"]["id"], version["id"]);
        assert_eq!(read.data["civitai"]["trainedWords"], version["trainedWords"]);
    }
}
