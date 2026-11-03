//! Read-only access to LoRA Manager's CivArchive metadata archive (`civitai/civitai.sqlite`).
//!
//! The archive has no index on `model_files.sha256`, so a lookup there is a full scan
//! (~0.36 s). lmx builds its own `sha256 -> (model, version)` table once (`data/archive_hash.sqlite`)
//! and rebuilds it when the archive file changes. The archive itself is never written.

use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Instant, UNIX_EPOCH},
};

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Value};

/// SHA256 of zero bytes: identifies no real file, but polluted archive rows carry it.
pub const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

pub struct Archive {
    src_path: PathBuf,
    src: Mutex<Connection>,
    idx: Mutex<Connection>,
    pub rows: u64,
    /// Seconds spent rebuilding the hash table on open (None when it was current).
    pub rebuilt_secs: Option<f64>,
}

fn stamp(p: &Path) -> anyhow::Result<String> {
    let md = std::fs::metadata(p)?;
    let m = md.modified()?.duration_since(UNIX_EPOCH)?.as_secs_f64();
    Ok(format!("{}:{m}", md.len()))
}

fn sha_blob(hex_str: &str) -> Option<[u8; 32]> {
    let mut out = [0u8; 32];
    hex::decode_to_slice(hex_str.trim(), &mut out).ok()?;
    Some(out)
}

impl Archive {
    /// Open `src` read-only and make sure the hash table in `idx_path` matches it.
    pub fn open(src: &Path, idx_path: &Path) -> anyhow::Result<Archive> {
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_URI;
        let src_conn = Connection::open_with_flags(src, flags)?;
        let mut idx = Connection::open(idx_path)?;
        idx.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS archive_hash (sha BLOB PRIMARY KEY, model_id INTEGER NOT NULL, version_id INTEGER NOT NULL) WITHOUT ROWID;",
        )?;
        let want = stamp(src)?;
        let have: Option<String> = idx.query_row("SELECT v FROM meta WHERE k = 'stamp'", [], |r| r.get(0)).optional()?;
        let mut rebuilt_secs = None;
        if have.as_deref() != Some(want.as_str()) {
            let t = Instant::now();
            let tx = idx.transaction()?;
            tx.execute("DELETE FROM archive_hash", [])?;
            {
                // Prefer the primary model file when several rows share a hash.
                let mut q = src_conn.prepare(
                    "SELECT sha256, model_id, version_id FROM model_files WHERE sha256 IS NOT NULL ORDER BY (type = 'Model') DESC, id ASC",
                )?;
                let mut ins = tx.prepare("INSERT OR IGNORE INTO archive_hash (sha, model_id, version_id) VALUES (?1, ?2, ?3)")?;
                let mut rows = q.query([])?;
                while let Some(r) = rows.next()? {
                    let s: String = r.get(0)?;
                    if s.eq_ignore_ascii_case(EMPTY_SHA256) {
                        continue;
                    }
                    if let Some(b) = sha_blob(&s) {
                        ins.execute(params![&b[..], r.get::<_, i64>(1)?, r.get::<_, i64>(2)?])?;
                    }
                }
            }
            tx.execute("INSERT OR REPLACE INTO meta (k, v) VALUES ('stamp', ?1)", params![want])?;
            tx.commit()?;
            rebuilt_secs = Some(t.elapsed().as_secs_f64());
        }
        let rows = idx.query_row("SELECT COUNT(*) FROM archive_hash", [], |r| r.get::<_, i64>(0))? as u64;
        Ok(Archive { src_path: src.to_path_buf(), src: Mutex::new(src_conn), idx: Mutex::new(idx), rows, rebuilt_secs })
    }

    pub fn path(&self) -> &Path {
        &self.src_path
    }

    /// (model_id, version_id) for a sha256 (any case).
    pub fn lookup(&self, sha: &str) -> Option<(i64, i64)> {
        if sha.eq_ignore_ascii_case(EMPTY_SHA256) {
            return None;
        }
        let b = sha_blob(sha)?;
        let idx = self.idx.lock().expect("archive idx lock");
        idx.query_row("SELECT model_id, version_id FROM archive_hash WHERE sha = ?1", params![&b[..]], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()
            .ok()
            .flatten()
    }

    /// Version payload shaped like Civitai's `/model-versions/{id}` (LoRA Manager
    /// `_get_version_with_model_data`), plus the archived `/models` object.
    pub fn version(&self, model_id: i64, version_id: i64) -> anyhow::Result<Option<(Value, Value)>> {
        let db = self.src.lock().expect("archive src lock");
        let v: Option<(String, String, String)> = db
            .query_row(
                "SELECT name, base_model, data FROM model_versions WHERE id = ?1 AND model_id = ?2",
                params![version_id, model_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((vname, base, vdata)) = v else { return Ok(None) };
        let m: Option<(String, String, String, Option<String>)> = db
            .query_row("SELECT name, type, data, username FROM models WHERE id = ?1", params![model_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .optional()?;
        let Some((mname, mtype, mdata, username)) = m else { return Ok(None) };
        let (Ok(vdata), Ok(mdata)) = (serde_json::from_str::<Value>(&vdata), serde_json::from_str::<Value>(&mdata)) else {
            return Ok(None);
        };
        let mut out = json!({
            "id": version_id,
            "modelId": model_id,
            "name": vname,
            "baseModel": base,
            "model": {
                "name": mname,
                "description": mdata.get("description").cloned().unwrap_or(Value::Null),
                "type": mtype,
                "tags": mdata.get("tags").cloned().unwrap_or(json!([])),
            },
            "creator": {
                "username": username.map(Value::String).or_else(|| mdata.pointer("/creator/username").cloned()).unwrap_or(Value::Null),
                "image": mdata.pointer("/creator/image").cloned().unwrap_or(Value::Null),
            },
            "source": "archive_db",
        });
        if let (Some(o), Some(vd)) = (out.as_object_mut(), vdata.as_object()) {
            for (k, val) in vd {
                o.insert(k.clone(), val.clone());
            }
        }
        let mut files: Vec<Value> = Vec::new();
        let mut q = db.prepare("SELECT data FROM model_files WHERE version_id = ?1 AND type = 'Model' ORDER BY id ASC")?;
        let rows = q.query_map(params![version_id], |r| r.get::<_, String>(0))?;
        for row in rows {
            if let Ok(Value::Object(mut f)) = serde_json::from_str::<Value>(&row?) {
                f.remove("modelId");
                f.remove("modelVersionId");
                files.push(Value::Object(f));
            }
        }
        let merged = match out.get("files").cloned() {
            Some(Value::Array(mut existing)) => {
                existing.extend(files);
                existing
            }
            Some(Value::Null) | None => files,
            Some(other) => {
                files.insert(0, other);
                files
            }
        };
        out["files"] = Value::Array(merged);
        // The /models object (for license fields, nsfw, poi) — mirrors what the API path gets.
        let mut model = mdata;
        if let Some(o) = model.as_object_mut() {
            o.insert("id".into(), json!(model_id));
        }
        Ok(Some((out, model)))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A tiny archive with the same schema as CivArchive's.
    pub fn make_archive(path: &Path) {
        let _ = std::fs::remove_file(path);
        let c = Connection::open(path).unwrap();
        c.execute_batch(
            "CREATE TABLE models (id INTEGER PRIMARY KEY, name TEXT NOT NULL, type TEXT NOT NULL, username TEXT, data TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
             CREATE TABLE model_versions (id INTEGER PRIMARY KEY, model_id INTEGER NOT NULL, position INTEGER NOT NULL, name TEXT NOT NULL, base_model TEXT NOT NULL, published_at INTEGER, data TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
             CREATE TABLE model_files (id INTEGER PRIMARY KEY, model_id INTEGER NOT NULL, version_id INTEGER NOT NULL, type TEXT NOT NULL, sha256 TEXT, data TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);",
        )
        .unwrap();
        c.execute(
            "INSERT INTO models VALUES (10, 'Archived Girl', 'LORA', 'alice', ?1, 0, 0)",
            params![r#"{"id":10,"name":"Archived Girl","description":"<p>desc</p>","tags":["character","anime"],"nsfw":false,"poi":false,"allowNoCredit":true,"allowCommercialUse":["Image"],"allowDerivatives":true,"allowDifferentLicense":true,"creator":{"username":"alice","image":null}}"#],
        )
        .unwrap();
        c.execute(
            "INSERT INTO model_versions VALUES (20, 10, 0, 'v1', 'Illustrious', 0, ?1, 0, 0)",
            params![r#"{"id":20,"modelId":10,"name":"v1","baseModel":"Illustrious","trainedWords":["archgirl"],"images":[{"url":"https://image.civitai.com/x/y/width=1024/1.jpeg","nsfwLevel":1,"type":"image"}],"downloadUrl":"https://civitai.com/api/download/models/20"}"#],
        )
        .unwrap();
        let sha = "AB".repeat(32);
        c.execute(
            "INSERT INTO model_files VALUES (30, 10, 20, 'Model', ?1, ?2, 0, 0)",
            params![sha, format!(r#"{{"id":30,"modelId":10,"modelVersionId":20,"name":"archived_girl.safetensors","sizeKB":10.5,"type":"Model","primary":true,"hashes":{{"SHA256":"{sha}","AutoV3":"ABC123"}}}}"#)],
        )
        .unwrap();
        c.execute("INSERT INTO model_files VALUES (31, 10, 20, 'Training Data', ?1, '{}', 0, 0)", params!["CD".repeat(32)]).unwrap();
        c.execute("INSERT INTO model_files VALUES (32, 10, 20, 'Model', ?1, '{}', 0, 0)", params![EMPTY_SHA256.to_uppercase()]).unwrap();
    }

    #[test]
    fn builds_hash_table_and_assembles_lm_shaped_payload() {
        let dir = std::env::temp_dir().join(format!("lmx-arch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("civitai.sqlite");
        make_archive(&src);
        let a = Archive::open(&src, &dir.join("idx.sqlite")).unwrap();
        assert_eq!(a.rows, 2, "empty-file hash skipped");
        assert_eq!(a.lookup(&"ab".repeat(32)), Some((10, 20)));
        assert_eq!(a.lookup(EMPTY_SHA256), None);
        assert_eq!(a.lookup("zz"), None);
        let (v, m) = a.version(10, 20).unwrap().unwrap();
        assert_eq!(v["source"], "archive_db");
        assert_eq!(v["model"]["name"], "Archived Girl");
        assert_eq!(v["creator"]["username"], "alice");
        assert_eq!(v["trainedWords"], json!(["archgirl"]));
        assert_eq!(v["files"].as_array().unwrap().len(), 2);
        assert!(v["files"][0].get("modelId").is_none());
        assert_eq!(m["allowCommercialUse"], json!(["Image"]));
        drop(a);
        // Reopen without changes: no rebuild needed, same answers.
        let a = Archive::open(&src, &dir.join("idx.sqlite")).unwrap();
        assert_eq!(a.lookup(&"AB".repeat(32)), Some((10, 20)));
    }
}
