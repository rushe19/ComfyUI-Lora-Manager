//! Path helpers. Stored paths use forward slashes, like LoRA Manager's catalog.

use std::path::{Path, PathBuf};

/// Preview file suffixes LoRA Manager looks for next to a model (`py/utils/constants.py`), in order.
pub const PREVIEW_SUFFIXES: &[&str] = &[
    ".webp", ".preview.webp", ".preview.png", ".preview.jpeg", ".preview.jpg", ".preview.mp4", ".png", ".jpeg",
    ".jpg", ".mp4", ".gif", ".webm", ".avif", ".jxl",
];

/// Normalize a path to a forward-slash string (no trailing slash).
pub fn norm(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    if s.len() > 3 && s.ends_with('/') {
        s.trim_end_matches('/').to_string()
    } else {
        s
    }
}

/// Model path without its extension (`H:/x/a.safetensors` -> `H:/x/a`).
pub fn stem_path(model: &Path) -> PathBuf {
    model.with_extension("")
}

/// `<stem>.metadata.json` next to the model (LoRA Manager "alongside" sidecar mode).
pub fn sidecar_path(model: &Path) -> PathBuf {
    append(&stem_path(model), ".metadata.json")
}

/// Append a raw suffix to a path (unlike `with_extension`, keeps dots in the stem intact).
pub fn append(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// Folder of `file` relative to `root`, forward slashes, "" for the root itself.
pub fn relative_folder(root: &Path, file: &Path) -> String {
    let parent = file.parent().unwrap_or(file);
    match parent.strip_prefix(root) {
        Ok(rel) => norm(rel),
        Err(_) => String::new(),
    }
}

pub fn is_video(path: &str) -> bool {
    let l = path.to_ascii_lowercase();
    l.ends_with(".mp4") || l.ends_with(".webm")
}

/// Make a remote file name safe for Windows: no separators/reserved characters, no trailing dots.
pub fn sanitize_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches(['.', ' ']);
    if trimmed.is_empty() || trimmed.chars().all(|c| c == '.') {
        "download.bin".into()
    } else {
        trimmed.to_string()
    }
}

/// File name for a download the user renamed: made safe like any file name, and the real extension
/// (`original`'s) is kept, so a `.safetensors` file can never end up with another extension.
/// `None` when nothing usable is left (empty, only the extension, or longer than 200 characters).
pub fn renamed_download(requested: &str, original: &str) -> Option<String> {
    let requested = requested.trim();
    if requested.trim_end_matches(['.', ' ']).is_empty() {
        return None;
    }
    let safe = sanitize_file_name(requested);
    let ext = Path::new(original).extension().and_then(|e| e.to_str()).unwrap_or("");
    let name = if ext.is_empty() || safe.to_lowercase().ends_with(&format!(".{}", ext.to_lowercase())) {
        safe
    } else {
        format!("{safe}.{ext}")
    };
    // Not `Path::file_stem`: it treats ".safetensors" as a hidden file whose stem is the whole name.
    let stem = if ext.is_empty() { name.as_str() } else { &name[..name.len() - ext.len() - 1] };
    if stem.trim_end_matches(['.', ' ']).is_empty() || name.chars().count() > 200 {
        return None;
    }
    Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_keeps_dots_in_stem() {
        let p = Path::new("H:/m/Lora/a.b.v1.safetensors");
        assert_eq!(norm(&sidecar_path(p)), "H:/m/Lora/a.b.v1.metadata.json");
    }

    #[test]
    fn relative_folder_works() {
        let root = Path::new("H:/m/Lora");
        assert_eq!(relative_folder(root, Path::new("H:/m/Lora/x.safetensors")), "");
        assert_eq!(relative_folder(root, Path::new("H:/m/Lora/Char/Sub/x.safetensors")), "Char/Sub");
    }

    #[test]
    fn sanitize_removes_reserved_characters() {
        assert_eq!(sanitize_file_name("a/b\\c:d*e?.safetensors"), "a_b_c_d_e_.safetensors");
        assert_eq!(sanitize_file_name("..\\..\\evil.safetensors"), ".._.._evil.safetensors");
        assert_eq!(sanitize_file_name(" name. "), "name");
        assert_eq!(sanitize_file_name(".."), "download.bin");
    }

    #[test]
    fn norm_converts_backslashes() {
        assert_eq!(norm(Path::new(r"H:\models\embeddings\")), "H:/models/embeddings");
    }

    #[test]
    fn renamed_download_keeps_the_real_extension() {
        let o = "orig.safetensors";
        assert_eq!(renamed_download("my lora", o).as_deref(), Some("my lora.safetensors"));
        assert_eq!(renamed_download("my lora.safetensors", o).as_deref(), Some("my lora.safetensors"));
        assert_eq!(renamed_download("My.SafeTensors", o).as_deref(), Some("My.SafeTensors"));
        assert_eq!(renamed_download("a.v2", o).as_deref(), Some("a.v2.safetensors"), "a dot in the name is not an extension");
        assert_eq!(renamed_download("evil.exe", o).as_deref(), Some("evil.exe.safetensors"));
    }

    #[test]
    fn renamed_download_is_safe_and_rejects_nothing_names() {
        let o = "orig.safetensors";
        assert_eq!(renamed_download("a/b\\c:d", o).as_deref(), Some("a_b_c_d.safetensors"), "no path separators");
        assert!(renamed_download("..", o).is_none(), "dots only");
        assert!(renamed_download("   ", o).is_none());
        assert!(renamed_download(".safetensors", o).is_none(), "only the extension");
        assert!(renamed_download(&"a".repeat(250), o).is_none(), "too long");
    }
}
