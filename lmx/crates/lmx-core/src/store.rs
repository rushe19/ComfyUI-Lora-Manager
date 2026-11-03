//! Persistent index (SQLite). It is a cache: everything can be rebuilt from the sidecars.

use std::{collections::HashMap, path::Path};

use rusqlite::{params, Connection};

use crate::{record::ModelRecord, scan::file_key};

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> anyhow::Result<Store> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS models (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 file_key TEXT NOT NULL UNIQUE,
                 record TEXT NOT NULL
             );",
        )?;
        Ok(Store { conn })
    }

    /// Load every record keyed by [`file_key`], ids filled from the table.
    pub fn load(&self) -> anyhow::Result<HashMap<String, ModelRecord>> {
        let mut stmt = self.conn.prepare("SELECT id, file_key, record FROM models")?;
        let mut out = HashMap::new();
        let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))?;
        for row in rows {
            let (id, key, json) = row?;
            match serde_json::from_str::<ModelRecord>(&json) {
                Ok(mut r) => {
                    r.id = id;
                    out.insert(key, r);
                }
                // A record from an older schema: drop it, the next scan re-parses that file.
                Err(_) => continue,
            }
        }
        Ok(out)
    }

    /// Replace the index with `records`; assigns ids to new records in place.
    /// Only rows whose content changed are written.
    pub fn sync(&mut self, previous: &HashMap<String, ModelRecord>, records: &mut [ModelRecord]) -> anyhow::Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut upsert = tx.prepare(
                "INSERT INTO models (file_key, record) VALUES (?1, ?2)
                 ON CONFLICT(file_key) DO UPDATE SET record = excluded.record",
            )?;
            let mut get_id = tx.prepare("SELECT id FROM models WHERE file_key = ?1")?;
            let mut keep = std::collections::HashSet::new();
            for r in records.iter_mut() {
                let key = file_key(r.model_type, &r.file_path);
                let changed = match previous.get(&key) {
                    Some(p) => {
                        let mut p = p.clone();
                        p.id = r.id;
                        p != *r
                    }
                    None => true,
                };
                if changed {
                    let mut stored = r.clone();
                    stored.id = 0;
                    upsert.execute(params![key, serde_json::to_string(&stored)?])?;
                    if r.id == 0 {
                        r.id = get_id.query_row(params![key], |row| row.get(0))?;
                    }
                }
                keep.insert(key);
            }
            let mut del = tx.prepare("DELETE FROM models WHERE file_key = ?1")?;
            for key in previous.keys() {
                if !keep.contains(key) {
                    del.execute(params![key])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model_type::ModelType, record::FileFacts};

    fn rec(name: &str) -> ModelRecord {
        let path = format!("H:/m/Lora/{name}.safetensors");
        let facts = FileFacts {
            model_type: ModelType::Lora,
            path: Path::new(&path),
            root: Path::new("H:/m/Lora"),
            size: 1,
            modified: 1.0,
            sidecar_mtime: None,
            sidecar_size: None,
        };
        ModelRecord::build(&facts, None, None)
    }

    #[test]
    fn roundtrip_assigns_stable_ids_and_deletes_removed() {
        let path = std::env::temp_dir().join(format!("lmx-store-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut store = Store::open(&path).unwrap();

        let mut first = vec![rec("a"), rec("b")];
        store.sync(&HashMap::new(), &mut first).unwrap();
        assert!(first.iter().all(|r| r.id > 0));
        let loaded = store.load().unwrap();
        assert_eq!(loaded.len(), 2);

        let a_id = first[0].id;
        let mut second = vec![loaded[&file_key(ModelType::Lora, "H:/m/Lora/a.safetensors")].clone(), rec("c")];
        second[0].model_name = "changed".into();
        store.sync(&loaded, &mut second).unwrap();

        let again = store.load().unwrap();
        assert_eq!(again.len(), 2);
        let a = &again[&file_key(ModelType::Lora, "H:/m/Lora/a.safetensors")];
        assert_eq!(a.id, a_id);
        assert_eq!(a.model_name, "changed");
        assert!(!again.contains_key(&file_key(ModelType::Lora, "H:/m/Lora/b.safetensors")));
    }
}
