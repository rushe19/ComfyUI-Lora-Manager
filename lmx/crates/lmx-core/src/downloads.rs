//! Persistent download queue (SQLite, `data/downloads.sqlite`).

use std::path::Path;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::model_type::ModelType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Done,
    Failed,
    Canceled,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            JobStatus::Queued => "queued",
            JobStatus::Running => "running",
            JobStatus::Done => "done",
            JobStatus::Failed => "failed",
            JobStatus::Canceled => "canceled",
        }
    }

    pub fn is_active(self) -> bool {
        matches!(self, JobStatus::Queued | JobStatus::Running)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DownloadJob {
    pub id: i64,
    pub created_at: f64,
    pub updated_at: f64,
    pub status: JobStatus,
    pub model_type: ModelType,
    pub model_id: i64,
    pub version_id: i64,
    pub file_id: Option<i64>,
    pub model_name: String,
    pub version_name: String,
    pub base_model: String,
    pub file_name: String,
    /// Absolute target folder (validated against configured roots at enqueue time).
    pub dest_dir: String,
    pub url: String,
    pub size_bytes: u64,
    pub sha256: Option<String>,
    pub bytes_done: u64,
    pub error: Option<String>,
    pub image_url: Option<String>,
    pub nsfw_level: i64,
    pub result_path: Option<String>,
    /// Civitai payloads captured at enqueue time (`{"model": .., "version": ..}`) for the sidecar.
    #[serde(default)]
    pub meta: Option<serde_json::Value>,
}

pub struct DownloadStore {
    conn: Connection,
}

impl DownloadStore {
    pub fn open(path: &Path) -> anyhow::Result<DownloadStore> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS downloads (id INTEGER PRIMARY KEY AUTOINCREMENT, job TEXT NOT NULL);",
        )?;
        Ok(DownloadStore { conn })
    }

    pub fn insert(&self, job: &mut DownloadJob) -> anyhow::Result<()> {
        self.conn.execute("INSERT INTO downloads (job) VALUES (?1)", params![serde_json::to_string(job)?])?;
        job.id = self.conn.last_insert_rowid();
        self.update(job)
    }

    pub fn update(&self, job: &DownloadJob) -> anyhow::Result<()> {
        self.conn.execute("UPDATE downloads SET job = ?2 WHERE id = ?1", params![job.id, serde_json::to_string(job)?])?;
        Ok(())
    }

    pub fn delete(&self, id: i64) -> anyhow::Result<()> {
        self.conn.execute("DELETE FROM downloads WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// All jobs, oldest first. Jobs left `running` by a crash come back as `queued` (they resume).
    pub fn load(&self) -> anyhow::Result<Vec<DownloadJob>> {
        let mut stmt = self.conn.prepare("SELECT id, job FROM downloads ORDER BY id")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (id, text) = row?;
            if let Ok(mut j) = serde_json::from_str::<DownloadJob>(&text) {
                j.id = id;
                if j.status == JobStatus::Running {
                    j.status = JobStatus::Queued;
                    self.update(&j)?;
                }
                out.push(j);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn job() -> DownloadJob {
        DownloadJob {
            id: 0,
            created_at: 1.0,
            updated_at: 1.0,
            status: JobStatus::Queued,
            model_type: ModelType::Lora,
            model_id: 1,
            version_id: 2,
            file_id: Some(3),
            model_name: "m".into(),
            version_name: "v1".into(),
            base_model: "Illustrious".into(),
            file_name: "m.safetensors".into(),
            dest_dir: "H:/m/Lora".into(),
            url: "https://civitai.com/api/download/models/2".into(),
            size_bytes: 10,
            sha256: Some("ab".into()),
            bytes_done: 0,
            error: None,
            image_url: None,
            nsfw_level: 0,
            result_path: None,
            meta: None,
        }
    }

    #[test]
    fn running_jobs_resume_as_queued_after_restart() {
        let p = std::env::temp_dir().join(format!("lmx-dl-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let s = DownloadStore::open(&p).unwrap();
        let mut a = job();
        s.insert(&mut a).unwrap();
        let mut b = job();
        s.insert(&mut b).unwrap();
        a.status = JobStatus::Running;
        s.update(&a).unwrap();
        b.status = JobStatus::Done;
        s.update(&b).unwrap();
        drop(s);
        let s = DownloadStore::open(&p).unwrap();
        let jobs = s.load().unwrap();
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].status, JobStatus::Queued);
        assert_eq!(jobs[1].status, JobStatus::Done);
        s.delete(jobs[1].id).unwrap();
        assert_eq!(s.load().unwrap().len(), 1);
    }
}
