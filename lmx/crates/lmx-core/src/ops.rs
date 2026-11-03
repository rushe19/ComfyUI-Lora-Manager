//! Filesystem operations on models: companions, move/rename (with sidecar fix-up), sidecar edits.
//!
//! Every changed sidecar is backed up first (see [`crate::sidecar::Backup`]); model files are
//! only ever renamed, never rewritten.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::{
    paths,
    sidecar::{self, Backup},
};

/// Files that belong to a model and travel with it: `<stem><suffix>`.
pub const COMPANION_SUFFIXES: &[&str] = &[
    ".metadata.json", ".civitai.info", ".json", ".txt", ".webp", ".preview.webp", ".preview.png", ".preview.jpeg",
    ".preview.jpg", ".preview.mp4", ".preview.gif", ".preview.webm", ".png", ".jpeg", ".jpg", ".mp4", ".gif", ".webm",
    ".avif", ".jxl",
];

pub fn companions(model: &Path) -> Vec<PathBuf> {
    let stem = paths::stem_path(model);
    COMPANION_SUFFIXES.iter().map(|s| paths::append(&stem, s)).filter(|p| p.is_file()).collect()
}

#[derive(Debug)]
pub enum OpError {
    Exists(PathBuf),
    NotFound(PathBuf),
    Invalid(String),
    Damaged,
    Io(std::io::Error),
}

impl std::fmt::Display for OpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpError::Exists(p) => write!(f, "移動先に同じ名前のファイルがあります: {}", paths::norm(p)),
            OpError::NotFound(p) => write!(f, "ファイルが見つかりません: {}", paths::norm(p)),
            OpError::Invalid(s) => write!(f, "{s}"),
            OpError::Damaged => write!(f, "メタデータファイルが壊れているため書き換えません（先に修復してください）"),
            OpError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for OpError {}

impl From<std::io::Error> for OpError {
    fn from(e: std::io::Error) -> Self {
        OpError::Io(e)
    }
}

fn valid_stem(s: &str) -> bool {
    !s.trim().is_empty() && s == paths::sanitize_file_name(s) && !s.starts_with('.')
}

/// Rename, falling back to copy + delete across volumes.
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(e) if e.raw_os_error() == Some(17) || e.kind() == std::io::ErrorKind::CrossesDevices => {
            let n = std::fs::copy(from, to)?;
            if n != std::fs::metadata(from)?.len() {
                let _ = std::fs::remove_file(to);
                return Err(std::io::Error::other("copy size mismatch"));
            }
            std::fs::remove_file(from)
        }
        Err(e) => Err(e),
    }
}

/// Move a model (and its companions) to `dest_dir`, optionally under a new stem.
/// The sidecar is updated (`file_name`, `file_path`, `preview_url`). Returns the new model path.
pub fn move_model(model: &Path, dest_dir: &Path, new_stem: Option<&str>, backup: Option<&Backup>) -> Result<PathBuf, OpError> {
    if !model.is_file() {
        return Err(OpError::NotFound(model.to_path_buf()));
    }
    let old_stem = model.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = model.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let stem = new_stem.unwrap_or(&old_stem);
    if !valid_stem(stem) {
        return Err(OpError::Invalid(format!("使えない名前です: {stem}")));
    }
    let new_model = dest_dir.join(format!("{stem}{ext}"));
    if paths::norm(&new_model).eq_ignore_ascii_case(&paths::norm(model)) && stem == old_stem {
        return Ok(new_model);
    }
    // Plan every move first; refuse if anything would be overwritten.
    let old_base = paths::stem_path(model);
    let new_base = dest_dir.join(stem);
    let mut plan: Vec<(PathBuf, PathBuf)> = vec![(model.to_path_buf(), new_model.clone())];
    for c in companions(model) {
        let name = c.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let suffix = &name[old_stem.len()..];
        plan.push((c, paths::append(&new_base, suffix)));
    }
    let case_only = |a: &Path, b: &Path| paths::norm(a).eq_ignore_ascii_case(&paths::norm(b));
    for (from, to) in &plan {
        if to.exists() && !case_only(from, to) {
            return Err(OpError::Exists(to.clone()));
        }
    }
    let old_sidecar = paths::sidecar_path(model);
    if let Some(b) = backup {
        b.save(&old_sidecar)?;
    }
    // Read the sidecar before moving so a damaged one stops the whole operation.
    let side = match sidecar::read(&old_sidecar) {
        Ok(Some(r)) if r.damage.is_some() => return Err(OpError::Damaged),
        Ok(r) => r.map(|r| r.data),
        Err(sidecar::SidecarError::Invalid(_)) => return Err(OpError::Damaged),
        Err(sidecar::SidecarError::Io(e)) => return Err(OpError::Io(e)),
    };
    std::fs::create_dir_all(dest_dir)?;
    let mut done: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (from, to) in &plan {
        if let Err(e) = move_file(from, to) {
            for (f, t) in done.iter().rev() {
                let _ = move_file(t, f);
            }
            return Err(OpError::Io(e));
        }
        done.push((from.clone(), to.clone()));
    }
    if let Some(mut map) = side {
        map.insert("file_name".into(), Value::String(stem.to_string()));
        map.insert("file_path".into(), Value::String(paths::norm(&new_model)));
        if let Some(p) = map.get("preview_url").and_then(Value::as_str).map(str::to_string) {
            let old_base_s = paths::norm(&old_base);
            let pn = p.replace('\\', "/");
            if pn.len() > old_base_s.len() && pn[..old_base_s.len()].eq_ignore_ascii_case(&old_base_s) {
                let moved = format!("{}{}", paths::norm(&new_base), &pn[old_base_s.len()..]);
                map.insert("preview_url".into(), Value::String(moved));
            }
        }
        sidecar::write(&paths::sidecar_path(&new_model), &map, None)?;
    }
    Ok(new_model)
}

/// Read-modify-write a sidecar. Refuses damaged files unless `allow_damaged` (the rescued
/// content is then written back, which repairs the file).
pub fn edit_sidecar<F>(model: &Path, backup: Option<&Backup>, allow_damaged: bool, f: F) -> Result<Map<String, Value>, OpError>
where
    F: FnOnce(&mut Map<String, Value>),
{
    let path = paths::sidecar_path(model);
    let mut map = match sidecar::read(&path) {
        Ok(Some(r)) if r.damage.is_some() && !allow_damaged => return Err(OpError::Damaged),
        Ok(Some(r)) => r.data,
        Ok(None) => return Err(OpError::NotFound(path)),
        Err(sidecar::SidecarError::Invalid(_)) => return Err(OpError::Damaged),
        Err(sidecar::SidecarError::Io(e)) => return Err(OpError::Io(e)),
    };
    f(&mut map);
    sidecar::write(&path, &map, backup)?;
    Ok(map)
}

/// LoRA Manager's default sidecar for a model without metadata (`create_default_metadata`).
pub fn default_sidecar(model: &Path, size: u64, modified: f64, sha256: &str, preview: Option<&str>) -> Map<String, Value> {
    let stem = model.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let v = serde_json::json!({
        "file_name": stem,
        "model_name": stem,
        "file_path": paths::norm(model),
        "size": size,
        "modified": modified,
        "sha256": sha256.to_ascii_lowercase(),
        "base_model": "Unknown",
        "preview_url": preview.unwrap_or(""),
        "preview_nsfw_level": 0,
        "notes": "",
        "from_civitai": true,
        "civitai": {},
        "tags": [],
        "modelDescription": "",
        "civitai_deleted": false,
        "favorite": false,
        "exclude": false,
        "db_checked": false,
        "skip_metadata_refresh": false,
        "metadata_source": null,
        "last_checked_at": 0,
        "hash_status": "completed",
        "autov3": null,
        "quantization": null,
        "usage_tips": "{}"
    });
    match v {
        Value::Object(m) => m,
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lmx-ops-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn seed(dir: &Path) -> PathBuf {
        let m = dir.join("Char/miku.safetensors");
        std::fs::create_dir_all(m.parent().unwrap()).unwrap();
        std::fs::write(&m, b"weights").unwrap();
        std::fs::write(dir.join("Char/miku.preview.png"), b"png").unwrap();
        std::fs::write(dir.join("Char/miku.civitai.info"), b"{}").unwrap();
        std::fs::write(dir.join("Char/mikuX.preview.png"), b"other model").unwrap();
        let side = serde_json::json!({
            "file_name": "miku", "model_name": "Miku", "file_path": paths::norm(&m),
            "preview_url": paths::norm(&dir.join("Char/miku.preview.png")), "notes": "n", "extra": [1, 2]
        });
        std::fs::write(dir.join("Char/miku.metadata.json"), serde_json::to_vec_pretty(&side).unwrap()).unwrap();
        m
    }

    #[test]
    fn move_and_rename_carry_companions_and_fix_sidecar() {
        let d = tmp("move");
        let m = seed(&d);
        let b = Backup { dir: d.join("bk") };
        let new = move_model(&m, &d.join("Anime/Vocaloid"), Some("hatsune_miku"), Some(&b)).unwrap();
        assert_eq!(new, d.join("Anime/Vocaloid/hatsune_miku.safetensors"));
        assert!(new.exists() && !m.exists());
        assert!(d.join("Anime/Vocaloid/hatsune_miku.preview.png").exists());
        assert!(d.join("Anime/Vocaloid/hatsune_miku.civitai.info").exists());
        assert!(d.join("Char/mikuX.preview.png").exists(), "a different model's file stays");
        let s = sidecar::read(&d.join("Anime/Vocaloid/hatsune_miku.metadata.json")).unwrap().unwrap().data;
        assert_eq!(s["file_name"], "hatsune_miku");
        assert_eq!(s["file_path"], paths::norm(&new));
        assert!(s["preview_url"].as_str().unwrap().ends_with("Anime/Vocaloid/hatsune_miku.preview.png"));
        assert_eq!(s["model_name"], "Miku");
        assert_eq!(s["extra"], serde_json::json!([1, 2]));
        assert!(b.target_for(&d.join("Char/miku.metadata.json")).exists(), "old sidecar backed up");
    }

    #[test]
    fn move_refuses_to_overwrite_and_leaves_everything_in_place() {
        let d = tmp("clash");
        let m = seed(&d);
        std::fs::create_dir_all(d.join("Other")).unwrap();
        std::fs::write(d.join("Other/miku.preview.png"), b"someone else's").unwrap();
        let e = move_model(&m, &d.join("Other"), None, None).unwrap_err();
        assert!(matches!(e, OpError::Exists(_)));
        assert!(m.exists() && d.join("Char/miku.metadata.json").exists());
        assert_eq!(std::fs::read(d.join("Other/miku.preview.png")).unwrap(), b"someone else's");
        assert!(matches!(move_model(&m, &d, Some("../evil"), None), Err(OpError::Invalid(_))));
    }

    #[test]
    fn edit_refuses_damaged_sidecar_unless_allowed() {
        let d = tmp("edit");
        let m = seed(&d);
        let out = edit_sidecar(&m, None, false, |s| {
            s.insert("favorite".into(), Value::Bool(true));
        })
        .unwrap();
        assert_eq!(out["favorite"], true);
        assert_eq!(out["notes"], "n");
        std::fs::write(d.join("Char/miku.metadata.json"), b"{\"a\": 1}}").unwrap();
        assert!(matches!(edit_sidecar(&m, None, false, |_| {}), Err(OpError::Damaged)));
        edit_sidecar(&m, None, true, |_| {}).unwrap();
        assert!(sidecar::read(&d.join("Char/miku.metadata.json")).unwrap().unwrap().damage.is_none());
    }
}
