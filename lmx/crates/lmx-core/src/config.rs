//! lmx configuration (`<data>/lmx.toml`), optionally importing roots from LoRA Manager's settings.json.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model_type::ModelType;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Roots {
    #[serde(default)]
    pub lora: Vec<PathBuf>,
    #[serde(default)]
    pub checkpoint: Vec<PathBuf>,
    #[serde(default)]
    pub embedding: Vec<PathBuf>,
    #[serde(default)]
    pub other: Vec<PathBuf>,
}

impl Roots {
    pub fn get(&self, t: ModelType) -> &[PathBuf] {
        match t {
            ModelType::Lora => &self.lora,
            ModelType::Checkpoint => &self.checkpoint,
            ModelType::Embedding => &self.embedding,
            ModelType::Other => &self.other,
        }
    }

    fn get_mut(&mut self, t: ModelType) -> &mut Vec<PathBuf> {
        match t {
            ModelType::Lora => &mut self.lora,
            ModelType::Checkpoint => &mut self.checkpoint,
            ModelType::Embedding => &mut self.embedding,
            ModelType::Other => &mut self.other,
        }
    }

    pub fn is_empty(&self) -> bool {
        ModelType::ALL.iter().all(|t| self.get(*t).is_empty())
    }

    fn push_unique(&mut self, t: ModelType, p: PathBuf) {
        let list = self.get_mut(t);
        let key = crate::paths::norm(&p).to_lowercase();
        if !list.iter().any(|x| crate::paths::norm(x).to_lowercase() == key) {
            list.push(p);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    #[serde(default)]
    pub roots: Roots,
    /// Optional LoRA Manager settings.json to import `folder_paths` from (merged with `roots`).
    #[serde(default)]
    pub lm_settings: Option<PathBuf>,
    /// Parallel workers for sidecar parsing (I/O-latency bound, so more than CPU-bound work).
    #[serde(default = "default_scan_threads")]
    pub scan_threads: usize,
    /// Host used for "open on Civitai" links (e.g. civitai.red for mature content).
    #[serde(default = "default_civitai_host")]
    pub civitai_host: String,
    /// Directory holding lmx.toml, index.sqlite and caches. Not serialized.
    #[serde(skip)]
    pub data_dir: PathBuf,
}

fn default_scan_threads() -> usize {
    16
}

fn default_civitai_host() -> String {
    "civitai.com".into()
}

impl Default for Config {
    fn default() -> Self {
        Config {
            roots: Roots::default(),
            lm_settings: None,
            scan_threads: default_scan_threads(),
            civitai_host: default_civitai_host(),
            data_dir: PathBuf::from("data"),
        }
    }
}

impl Config {
    /// Load `<data_dir>/lmx.toml` (defaults when missing) and merge LoRA Manager roots if configured.
    pub fn load(data_dir: &Path) -> anyhow::Result<Config> {
        let file = data_dir.join("lmx.toml");
        let mut cfg: Config = match std::fs::read_to_string(&file) {
            Ok(text) => toml::from_str(&text).map_err(|e| anyhow::anyhow!("{}: {e}", file.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
            Err(e) => return Err(e.into()),
        };
        cfg.data_dir = data_dir.to_path_buf();
        if let Some(lm) = cfg.lm_settings.clone() {
            let lm = if lm.is_relative() { data_dir.join(lm) } else { lm };
            let imported = roots_from_lm_settings(&std::fs::read_to_string(&lm)?)?;
            for t in ModelType::ALL {
                for p in imported.get(t) {
                    cfg.roots.push_unique(t, p.clone());
                }
            }
        }
        Ok(cfg)
    }

    pub fn index_path(&self) -> PathBuf {
        self.data_dir.join("index.sqlite")
    }
}

/// Map LoRA Manager `folder_paths` to lmx roots.
/// loras -> lora; checkpoints + unet -> checkpoint; embeddings -> embedding;
/// vae / upscale_models / text_encoders / clip -> other.
pub fn roots_from_lm_settings(json_text: &str) -> anyhow::Result<Roots> {
    let v: serde_json::Value = serde_json::from_str(json_text)?;
    let mut roots = Roots::default();
    let Some(fp) = v.get("folder_paths").and_then(|x| x.as_object()) else {
        return Ok(roots);
    };
    for (key, list) in fp {
        let t = match key.as_str() {
            "loras" => ModelType::Lora,
            "checkpoints" | "unet" => ModelType::Checkpoint,
            "embeddings" => ModelType::Embedding,
            "vae" | "upscale_models" | "text_encoders" | "clip" => ModelType::Other,
            _ => continue,
        };
        if let Some(arr) = list.as_array() {
            for p in arr.iter().filter_map(|x| x.as_str()) {
                roots.push_unique(t, PathBuf::from(p));
            }
        }
    }
    Ok(roots)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_lm_folder_paths() {
        let text = r#"{"folder_paths": {
            "loras": ["H:/m/Lora"], "checkpoints": ["H:/m/SD"], "unet": ["H:\\m\\SD\\diffusion_models"],
            "embeddings": ["H:\\emb"], "vae": ["H:/m/VAE"], "upscale_models": [], "custom": ["X:/ignored"]
        }}"#;
        let r = roots_from_lm_settings(text).unwrap();
        assert_eq!(r.lora, vec![PathBuf::from("H:/m/Lora")]);
        assert_eq!(r.checkpoint.len(), 2);
        assert_eq!(r.embedding, vec![PathBuf::from("H:\\emb")]);
        assert_eq!(r.other, vec![PathBuf::from("H:/m/VAE")]);
    }

    #[test]
    fn load_merges_toml_roots_and_lm_settings_without_duplicates() {
        let dir = std::env::temp_dir().join(format!("lmx-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("lm.json"), r#"{"folder_paths":{"loras":["H:/m/Lora","H:/extra"]}}"#).unwrap();
        std::fs::write(
            dir.join("lmx.toml"),
            "lm_settings = \"lm.json\"\nscan_threads = 4\n[roots]\nlora = [\"H:\\\\m\\\\Lora\"]\n",
        )
        .unwrap();
        let cfg = Config::load(&dir).unwrap();
        assert_eq!(cfg.scan_threads, 4);
        assert_eq!(cfg.roots.lora.len(), 2, "{:?}", cfg.roots.lora);
    }

    #[test]
    fn missing_config_gives_defaults() {
        let dir = std::env::temp_dir().join(format!("lmx-cfg-empty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = Config::load(&dir).unwrap();
        assert!(cfg.roots.is_empty());
        assert_eq!(cfg.scan_threads, 16);
    }
}
