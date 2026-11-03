//! The in-memory / indexed view of one model file.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{model_type::ModelType, paths, sidecar::Damage};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRecord {
    /// Stable id assigned by the store (0 until stored).
    pub id: i64,
    pub model_type: ModelType,
    /// Absolute model path, forward slashes (business path, symlinks not resolved).
    pub file_path: String,
    /// Library root this file was found under.
    pub root: String,
    /// Folder relative to `root` ("" = root).
    pub folder: String,
    /// File stem (LoRA Manager's `file_name`, used in `<lora:file_name:1>`).
    pub file_name: String,
    pub model_name: String,
    pub base_model: String,
    pub size: u64,
    /// Model file mtime (unix seconds).
    pub modified: f64,
    pub sha256: Option<String>,
    pub preview_path: Option<String>,
    pub preview_nsfw_level: i64,
    pub favorite: bool,
    pub exclude: bool,
    pub tags: Vec<String>,
    pub trained_words: Vec<String>,
    pub creator: Option<String>,
    pub civitai_model_id: Option<i64>,
    pub civitai_version_id: Option<i64>,
    pub metadata_source: Option<String>,
    pub has_sidecar: bool,
    pub from_civitai: bool,
    pub damage: Option<Damage>,
    /// Sidecar error text when it could not be read at all.
    pub sidecar_error: Option<String>,
    // Change detection
    pub sidecar_mtime: Option<f64>,
    pub sidecar_size: Option<u64>,
}

/// Facts about the files on disk, gathered by the scanner.
#[derive(Debug, Clone)]
pub struct FileFacts<'a> {
    pub model_type: ModelType,
    pub path: &'a Path,
    pub root: &'a Path,
    pub size: u64,
    pub modified: f64,
    pub sidecar_mtime: Option<f64>,
    pub sidecar_size: Option<u64>,
}

fn s(map: &Map<String, Value>, key: &str) -> Option<String> {
    map.get(key).and_then(Value::as_str).map(str::to_string).filter(|v| !v.is_empty())
}

fn b(map: &Map<String, Value>, key: &str) -> bool {
    map.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn i(map: &Map<String, Value>, key: &str) -> Option<i64> {
    // Real sidecars mix ints and floats for the same key; accept both.
    map.get(key).and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
}

fn str_list(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|x| x.as_str())
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

impl ModelRecord {
    /// Build a record from file facts plus an optional parsed sidecar.
    pub fn build(f: &FileFacts, sidecar: Option<(&Map<String, Value>, Option<Damage>)>, sidecar_error: Option<String>) -> Self {
        let stem = f.path.file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
        let mut r = ModelRecord {
            id: 0,
            model_type: f.model_type,
            file_path: paths::norm(f.path),
            root: paths::norm(f.root),
            folder: paths::relative_folder(f.root, f.path),
            file_name: stem.clone(),
            model_name: stem,
            base_model: "Unknown".into(),
            size: f.size,
            modified: f.modified,
            sha256: None,
            preview_path: None,
            preview_nsfw_level: 0,
            favorite: false,
            exclude: false,
            tags: Vec::new(),
            trained_words: Vec::new(),
            creator: None,
            civitai_model_id: None,
            civitai_version_id: None,
            metadata_source: None,
            has_sidecar: false,
            from_civitai: false,
            damage: None,
            sidecar_error,
            sidecar_mtime: f.sidecar_mtime,
            sidecar_size: f.sidecar_size,
        };
        if let Some((m, damage)) = sidecar {
            r.has_sidecar = true;
            r.damage = damage;
            if let Some(v) = s(m, "model_name") {
                r.model_name = v;
            }
            if let Some(v) = s(m, "base_model") {
                r.base_model = v;
            }
            r.sha256 = s(m, "sha256").map(|h| h.to_ascii_lowercase());
            r.preview_path = s(m, "preview_url").map(|p| p.replace('\\', "/"));
            r.preview_nsfw_level = i(m, "preview_nsfw_level").unwrap_or(0);
            r.favorite = b(m, "favorite");
            r.exclude = b(m, "exclude");
            r.from_civitai = b(m, "from_civitai");
            r.tags = str_list(m.get("tags"));
            r.metadata_source = s(m, "metadata_source");
            if let Some(Value::Object(c)) = m.get("civitai") {
                r.trained_words = str_list(c.get("trainedWords"));
                r.civitai_version_id = i(c, "id");
                r.civitai_model_id = i(c, "modelId");
                r.creator = c
                    .get("creator")
                    .and_then(|x| x.get("username"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
        }
        r
    }

    /// Lowercased text used by local search.
    pub fn haystack(&self) -> String {
        let mut h = String::with_capacity(128);
        for part in [&self.model_name, &self.file_name, &self.folder, &self.base_model] {
            h.push_str(part);
            h.push('\n');
        }
        for t in self.tags.iter().chain(self.trained_words.iter()) {
            h.push_str(t);
            h.push('\n');
        }
        if let Some(c) = &self.creator {
            h.push_str(c);
        }
        h.to_lowercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn facts(path: &Path) -> FileFacts<'_> {
        FileFacts {
            model_type: ModelType::Lora,
            path,
            root: Path::new("H:/m/Lora"),
            size: 10,
            modified: 1.5,
            sidecar_mtime: Some(2.0),
            sidecar_size: Some(100),
        }
    }

    #[test]
    fn without_sidecar_uses_file_stem() {
        let p = Path::new("H:/m/Lora/Char/miku.v2.safetensors");
        let r = ModelRecord::build(&facts(p), None, None);
        assert_eq!(r.file_name, "miku.v2");
        assert_eq!(r.model_name, "miku.v2");
        assert_eq!(r.folder, "Char");
        assert_eq!(r.base_model, "Unknown");
        assert!(!r.has_sidecar);
    }

    #[test]
    fn reads_real_sidecar_shape_with_mixed_number_types() {
        let p = Path::new("H:/m/Lora/x.safetensors");
        let v = json!({
            "file_name": "x", "model_name": "Pretty X", "size": 123.0, "modified": 1.0,
            "sha256": "ABCDEF", "base_model": "Illustrious", "preview_url": "H:\\m\\Lora\\x.webp",
            "preview_nsfw_level": 4.0, "favorite": true, "tags": ["girl", " ", "style"],
            "metadata_source": null, "unknown_future_key": {"kept": true},
            "civitai": {"id": 22, "modelId": 11, "trainedWords": ["xtrigger"], "creator": {"username": "maker"}}
        });
        let m = v.as_object().unwrap();
        let r = ModelRecord::build(&facts(p), Some((m, None)), None);
        assert_eq!(r.model_name, "Pretty X");
        assert_eq!(r.sha256.as_deref(), Some("abcdef"));
        assert_eq!(r.preview_path.as_deref(), Some("H:/m/Lora/x.webp"));
        assert_eq!(r.preview_nsfw_level, 4);
        assert!(r.favorite);
        assert_eq!(r.tags, vec!["girl", "style"]);
        assert_eq!(r.metadata_source, None);
        assert_eq!(r.trained_words, vec!["xtrigger"]);
        assert_eq!((r.civitai_model_id, r.civitai_version_id), (Some(11), Some(22)));
        assert_eq!(r.creator.as_deref(), Some("maker"));
        assert!(r.haystack().contains("xtrigger"));
        assert!(r.haystack().contains("pretty x"));
    }
}
