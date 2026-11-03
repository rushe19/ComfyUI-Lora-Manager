//! Shared server state and the background scan job.

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, RwLock,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use lmx_core::{
    scan::{self, Progress, ScanStats},
    store::Store,
    Config, Library,
};
use serde::Serialize;

pub struct AppState {
    pub cfg: Config,
    pub library: RwLock<Arc<Library>>,
    pub scanning: AtomicBool,
    pub progress: Arc<Progress>,
    pub last_scan: Mutex<Option<LastScan>>,
    pub thumbs_dir: PathBuf,
    /// Limits concurrent thumbnail generation.
    pub thumb_permits: tokio::sync::Semaphore,
}

#[derive(Debug, Clone, Serialize)]
pub struct LastScan {
    pub finished_at: f64,
    pub stats: ScanStats,
    pub error: Option<String>,
}

impl AppState {
    /// Open the index and load the last known library (fast; no filesystem walk).
    pub fn open(cfg: Config) -> anyhow::Result<Arc<AppState>> {
        let store = Store::open(&cfg.index_path())?;
        let records: Vec<_> = store.load()?.into_values().collect();
        let thumbs_dir = cfg.data_dir.join("thumbs");
        std::fs::create_dir_all(&thumbs_dir)?;
        Ok(Arc::new(AppState {
            library: RwLock::new(Arc::new(Library::new(records))),
            cfg,
            scanning: AtomicBool::new(false),
            progress: Arc::new(Progress::default()),
            last_scan: Mutex::new(None),
            thumbs_dir,
            thumb_permits: tokio::sync::Semaphore::new(4),
        }))
    }

    pub fn library(&self) -> Arc<Library> {
        self.library.read().expect("library lock").clone()
    }

    /// Start an incremental scan in the background. Returns false if one is already running.
    pub fn spawn_scan(self: &Arc<Self>) -> bool {
        if self.scanning.swap(true, Ordering::SeqCst) {
            return false;
        }
        let state = self.clone();
        tokio::task::spawn_blocking(move || {
            let result = state.run_scan();
            let (stats, error) = match result {
                Ok(stats) => (stats, None),
                Err(e) => (ScanStats::default(), Some(e.to_string())),
            };
            if let Some(err) = &error {
                tracing::error!("scan failed: {err}");
            } else {
                tracing::info!(
                    "scan done: {} models ({} unchanged, {} parsed, {} removed, {} damaged) in {:.1}s",
                    stats.models, stats.unchanged, stats.parsed, stats.removed, stats.damaged, stats.total_secs
                );
            }
            let finished_at = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
            *state.last_scan.lock().expect("last_scan lock") = Some(LastScan { finished_at, stats, error });
            state.scanning.store(false, Ordering::SeqCst);
        });
        true
    }

    fn run_scan(&self) -> anyhow::Result<ScanStats> {
        for c in [&self.progress.walked_files, &self.progress.to_parse, &self.progress.parsed] {
            c.store(0, Ordering::Relaxed);
        }
        let previous = self.library().previous_map();
        let (mut records, stats) = scan::scan(&self.cfg, &previous, &self.progress);
        let mut store = Store::open(&self.cfg.index_path())?;
        store.sync(&previous, &mut records)?;
        *self.library.write().expect("library lock") = Arc::new(Library::new(records));
        Ok(stats)
    }
}
