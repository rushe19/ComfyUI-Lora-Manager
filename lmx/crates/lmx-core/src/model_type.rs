use serde::{Deserialize, Serialize};

/// Model categories, mirroring LoRA Manager's scanners.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum ModelType {
    Lora,
    Checkpoint,
    Embedding,
    Other,
}

/// Extensions accepted by LoRA Manager's CheckpointScanner / OtherScanner.
const PT_EXTENSIONS: &[&str] = &[
    "ckpt", "pt", "pt2", "bin", "pth", "safetensors", "pkl", "sft", "gguf",
];
/// EmbeddingScanner: same set without gguf.
const EMBEDDING_EXTENSIONS: &[&str] = &["ckpt", "pt", "pt2", "bin", "pth", "safetensors", "pkl", "sft"];
/// LoraScanner only accepts safetensors.
const LORA_EXTENSIONS: &[&str] = &["safetensors"];

impl ModelType {
    pub const ALL: [ModelType; 4] = [ModelType::Lora, ModelType::Checkpoint, ModelType::Embedding, ModelType::Other];

    pub fn as_str(self) -> &'static str {
        match self {
            ModelType::Lora => "lora",
            ModelType::Checkpoint => "checkpoint",
            ModelType::Embedding => "embedding",
            ModelType::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Option<ModelType> {
        match s.to_ascii_lowercase().as_str() {
            "lora" | "loras" => Some(ModelType::Lora),
            "checkpoint" | "checkpoints" => Some(ModelType::Checkpoint),
            "embedding" | "embeddings" => Some(ModelType::Embedding),
            "other" | "others" => Some(ModelType::Other),
            _ => None,
        }
    }

    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            ModelType::Lora => LORA_EXTENSIONS,
            ModelType::Embedding => EMBEDDING_EXTENSIONS,
            ModelType::Checkpoint | ModelType::Other => PT_EXTENSIONS,
        }
    }

    pub fn accepts(self, ext: &str) -> bool {
        let ext = ext.to_ascii_lowercase();
        self.extensions().iter().any(|e| *e == ext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lora_only_accepts_safetensors() {
        assert!(ModelType::Lora.accepts("SAFETENSORS"));
        assert!(!ModelType::Lora.accepts("pt"));
        assert!(ModelType::Embedding.accepts("pt"));
        assert!(!ModelType::Embedding.accepts("gguf"));
        assert!(ModelType::Checkpoint.accepts("gguf"));
    }

    #[test]
    fn parse_accepts_plural_route_names() {
        assert_eq!(ModelType::parse("loras"), Some(ModelType::Lora));
        assert_eq!(ModelType::parse("Checkpoint"), Some(ModelType::Checkpoint));
        assert_eq!(ModelType::parse("nope"), None);
    }
}
