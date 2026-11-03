//! Library scanning: walk the roots, detect changes by size/mtime, parse changed sidecars in parallel.
//!
//! Read-only: the scanner never writes into model folders.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use rayon::prelude::*;
use serde::Serialize;

use crate::{
    config::Config,
    model_type::ModelType,
    paths,
    record::{FileFacts, ModelRecord},
    sidecar,
};

#[derive(Debug, Default)]
pub struct Progress {
    pub walked_files: AtomicUsize,
    pub to_parse: AtomicUsize,
    pub parsed: AtomicUsize,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct ScanStats {
    pub models: usize,
    pub unchanged: usize,
    pub parsed: usize,
    pub removed: usize,
    pub damaged: usize,
    pub sidecar_errors: usize,
    pub missing_sidecars: usize,
    pub walk_secs: f64,
    pub parse_secs: f64,
    pub total_secs: f64,
}

#[derive(Debug, Clone, Copy)]
struct FileInfo {
    size: u64,
    mtime: f64,
}

fn mtime_secs(t: std::io::Result<SystemTime>) -> f64 {
    t.ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Key used to match files across scans (case-insensitive on Windows-style paths).
pub fn file_key(t: ModelType, path: &str) -> String {
    format!("{}|{}", t.as_str(), path.to_lowercase())
}

struct Candidate {
    model_type: ModelType,
    root: PathBuf,
    path: PathBuf,
    info: FileInfo,
}

/// Scan all configured roots. `previous` holds the last known records keyed by [`file_key`].
/// Returns the full current set of records (ids preserved from `previous`).
pub fn scan(cfg: &Config, previous: &HashMap<String, ModelRecord>, progress: &Progress) -> (Vec<ModelRecord>, ScanStats) {
    let start = Instant::now();
    let mut stats = ScanStats::default();

    // 1) Walk every root once; remember every file's size/mtime (sidecars and previews included),
    //    so later lookups need no extra filesystem calls.
    let mut files: HashMap<String, FileInfo> = HashMap::new();
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for t in ModelType::ALL {
        for root in cfg.roots.get(t) {
            for entry in walkdir::WalkDir::new(root).follow_links(true).into_iter().filter_map(Result::ok) {
                if !entry.file_type().is_file() {
                    continue;
                }
                let Ok(md) = entry.metadata() else { continue };
                let info = FileInfo { size: md.len(), mtime: mtime_secs(md.modified()) };
                progress.walked_files.fetch_add(1, Ordering::Relaxed);
                let path = entry.path();
                files.insert(paths::norm(path).to_lowercase(), info);
                let is_model = path.extension().and_then(|e| e.to_str()).is_some_and(|e| t.accepts(e));
                if is_model && seen.insert(file_key(t, &paths::norm(path))) {
                    candidates.push(Candidate { model_type: t, root: root.clone(), path: path.to_path_buf(), info });
                }
            }
        }
    }
    stats.walk_secs = start.elapsed().as_secs_f64();

    // 2) Decide which candidates need their sidecar parsed.
    let lookup = |p: &Path| files.get(&paths::norm(p).to_lowercase()).copied();
    let mut reused: Vec<ModelRecord> = Vec::new();
    let mut work: Vec<(&Candidate, Option<FileInfo>)> = Vec::new();
    for c in &candidates {
        let side = lookup(&paths::sidecar_path(&c.path));
        let key = file_key(c.model_type, &paths::norm(&c.path));
        match previous.get(&key) {
            Some(prev)
                if prev.size == c.info.size
                    && prev.modified == c.info.mtime
                    && prev.sidecar_mtime == side.map(|s| s.mtime)
                    && prev.sidecar_size == side.map(|s| s.size)
                    && prev.root == paths::norm(&c.root) =>
            {
                reused.push(prev.clone())
            }
            _ => work.push((c, side)),
        }
    }
    stats.unchanged = reused.len();
    progress.to_parse.store(work.len(), Ordering::Relaxed);

    // 3) Parse changed sidecars in parallel (latency bound on cold caches, so use many workers).
    let parse_start = Instant::now();
    let pool = rayon::ThreadPoolBuilder::new().num_threads(cfg.scan_threads.max(1)).build().expect("thread pool");
    let parsed: Vec<ModelRecord> = pool.install(|| {
        work.par_iter()
            .map(|(c, side)| {
                let facts = FileFacts {
                    model_type: c.model_type,
                    path: &c.path,
                    root: &c.root,
                    size: c.info.size,
                    modified: c.info.mtime,
                    sidecar_mtime: side.map(|s| s.mtime),
                    sidecar_size: side.map(|s| s.size),
                };
                let rec = if side.is_some() {
                    match sidecar::read(&paths::sidecar_path(&c.path)) {
                        Ok(Some(sc)) => ModelRecord::build(&facts, Some((&sc.data, sc.damage)), None),
                        Ok(None) => ModelRecord::build(&facts, None, None),
                        Err(e) => ModelRecord::build(&facts, None, Some(e.to_string())),
                    }
                } else {
                    ModelRecord::build(&facts, None, None)
                };
                progress.parsed.fetch_add(1, Ordering::Relaxed);
                rec
            })
            .collect()
    });
    stats.parse_secs = parse_start.elapsed().as_secs_f64();
    stats.parsed = parsed.len();

    // 4) Merge, keep ids, resolve previews against the walked file set.
    let mut out: Vec<ModelRecord> = reused;
    for mut r in parsed {
        if let Some(prev) = previous.get(&file_key(r.model_type, &r.file_path)) {
            r.id = prev.id;
        }
        out.push(r);
    }
    for r in &mut out {
        r.preview_path = resolve_preview(&r.file_path, r.preview_path.as_deref(), &files);
        if r.damage.is_some() {
            stats.damaged += 1;
        }
        if r.sidecar_error.is_some() {
            stats.sidecar_errors += 1;
        }
        if !r.has_sidecar && r.sidecar_error.is_none() {
            stats.missing_sidecars += 1;
        }
    }
    let current: HashSet<String> = out.iter().map(|r| file_key(r.model_type, &r.file_path)).collect();
    stats.removed = previous.keys().filter(|k| !current.contains(*k)).count();
    stats.models = out.len();
    stats.total_secs = start.elapsed().as_secs_f64();
    (out, stats)
}

/// Prefer the sidecar's preview if it exists; otherwise probe LoRA Manager's preview suffixes.
fn resolve_preview(model_path: &str, sidecar_preview: Option<&str>, files: &HashMap<String, FileInfo>) -> Option<String> {
    if let Some(p) = sidecar_preview {
        if files.contains_key(&p.to_lowercase()) || Path::new(p).is_file() {
            return Some(p.to_string());
        }
    }
    let stem = paths::norm(&paths::stem_path(Path::new(model_path)));
    for suffix in paths::PREVIEW_SUFFIXES {
        let candidate = format!("{stem}{suffix}");
        if files.contains_key(&candidate.to_lowercase()) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Roots;
    use sha2::{Digest, Sha256};

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lmx-scan-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn cfg_for(root: &Path) -> Config {
        Config {
            roots: Roots { lora: vec![root.to_path_buf()], ..Default::default() },
            scan_threads: 4,
            ..Default::default()
        }
    }

    fn by_name(recs: &[ModelRecord]) -> HashMap<String, ModelRecord> {
        recs.iter().map(|r| (r.file_name.clone(), r.clone())).collect()
    }

    fn prev_map(recs: &[ModelRecord]) -> HashMap<String, ModelRecord> {
        recs.iter()
            .enumerate()
            .map(|(i, r)| {
                let mut r = r.clone();
                r.id = i as i64 + 1;
                (file_key(r.model_type, &r.file_path), r)
            })
            .collect()
    }

    fn snapshot(dir: &Path) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = walkdir::WalkDir::new(dir)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .map(|e| (paths::norm(e.path()), hex::encode(Sha256::digest(std::fs::read(e.path()).unwrap()))))
            .collect();
        v.sort();
        v
    }

    #[test]
    fn scan_reads_sidecars_previews_and_damage_without_writing() {
        let root = tmp("basic");
        std::fs::create_dir_all(root.join("Char")).unwrap();
        std::fs::write(root.join("Char/miku.safetensors"), b"x").unwrap();
        std::fs::write(
            root.join("Char/miku.metadata.json"),
            br#"{"model_name":"Miku","base_model":"Illustrious","civitai":{"trainedWords":["hatsune miku"]}}"#,
        )
        .unwrap();
        std::fs::write(root.join("Char/miku.webp"), b"img").unwrap();
        std::fs::write(root.join("broken.safetensors"), b"x").unwrap();
        std::fs::write(root.join("broken.metadata.json"), b"{\"model_name\":\"B\"}}").unwrap();
        std::fs::write(root.join("bare.safetensors"), b"x").unwrap();
        std::fs::write(root.join("bare.preview.png"), b"img").unwrap();
        std::fs::write(root.join("not_a_lora.pt"), b"x").unwrap();
        let before = snapshot(&root);

        let (recs, stats) = scan(&cfg_for(&root), &HashMap::new(), &Progress::default());
        assert_eq!(stats.models, 3, "{recs:?}");
        let m = by_name(&recs);
        assert_eq!(m["miku"].model_name, "Miku");
        assert_eq!(m["miku"].folder, "Char");
        assert_eq!(m["miku"].trained_words, vec!["hatsune miku"]);
        assert!(m["miku"].preview_path.as_deref().unwrap().ends_with("Char/miku.webp"));
        assert_eq!(m["broken"].model_name, "B");
        assert!(m["broken"].damage.is_some());
        assert!(!m["bare"].has_sidecar);
        assert!(m["bare"].preview_path.as_deref().unwrap().ends_with("bare.preview.png"));
        assert_eq!(stats.damaged, 1);
        assert_eq!(stats.missing_sidecars, 1);
        assert_eq!(snapshot(&root), before, "scan must not modify any file");
    }

    #[test]
    fn rescan_reuses_unchanged_and_detects_changes_and_removals() {
        let root = tmp("incr");
        for n in ["a", "b", "c"] {
            std::fs::write(root.join(format!("{n}.safetensors")), b"x").unwrap();
            std::fs::write(root.join(format!("{n}.metadata.json")), format!(r#"{{"model_name":"{n}"}}"#)).unwrap();
        }
        let cfg = cfg_for(&root);
        let (first, _) = scan(&cfg, &HashMap::new(), &Progress::default());
        let prev = prev_map(&first);

        // change b's sidecar (size changes), remove c, add d
        std::fs::write(root.join("b.metadata.json"), r#"{"model_name":"b renamed"}"#).unwrap();
        std::fs::remove_file(root.join("c.safetensors")).unwrap();
        std::fs::write(root.join("d.safetensors"), b"x").unwrap();

        let (second, stats) = scan(&cfg, &prev, &Progress::default());
        assert_eq!(stats.unchanged, 1, "{stats:?}");
        assert_eq!(stats.parsed, 2);
        assert_eq!(stats.removed, 1);
        let m = by_name(&second);
        assert_eq!(m["b"].model_name, "b renamed");
        assert!(m.contains_key("d") && !m.contains_key("c"));
        let old_b = prev.values().find(|r| r.file_name == "b").unwrap();
        assert_eq!(m["b"].id, old_b.id, "id preserved across sidecar change");
        assert_eq!(m["d"].id, 0, "new file has no id yet");
    }
}
