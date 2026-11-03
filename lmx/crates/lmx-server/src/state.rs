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
    /// A scan was requested while one was running; run again when it finishes.
    pub rescan_pending: AtomicBool,
    pub progress: Arc<Progress>,
    pub last_scan: Mutex<Option<LastScan>>,
    pub thumbs_dir: PathBuf,
    /// Limits concurrent thumbnail generation.
    pub thumb_permits: tokio::sync::Semaphore,
    pub civitai: lmx_civitai::CivitaiClient,
    pub downloads: crate::downloads::Downloads,
    /// Local CivArchive metadata archive, opened on first use.
    pub archive: RwLock<Option<Arc<lmx_civitai::archive::Archive>>>,
    pub meta: crate::metadata::MetaJobs,
    /// Serializes sidecar writes made by lmx (edits, metadata job, moves).
    pub writes: tokio::sync::Mutex<()>,
    pub search: crate::search_prefs::SearchState,
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
        let civitai = match &cfg.civitai_api_base {
            Some(base) => lmx_civitai::CivitaiClient::with_api_base(base, cfg.civitai_api_key.clone())?,
            None => lmx_civitai::CivitaiClient::new(&cfg.civitai_host, cfg.civitai_api_key.clone())?,
        };
        let downloads = crate::downloads::Downloads::open(&cfg.data_dir.join("downloads.sqlite"))?;
        Ok(Arc::new(AppState {
            library: RwLock::new(Arc::new(Library::new(records))),
            cfg,
            scanning: AtomicBool::new(false),
            rescan_pending: AtomicBool::new(false),
            progress: Arc::new(Progress::default()),
            last_scan: Mutex::new(None),
            thumbs_dir,
            thumb_permits: tokio::sync::Semaphore::new(4),
            civitai,
            downloads,
            archive: RwLock::new(None),
            meta: Default::default(),
            writes: tokio::sync::Mutex::new(()),
            search: Default::default(),
        }))
    }

    /// Scan now, or right after the running scan finishes.
    pub fn request_scan(self: &Arc<Self>) {
        if !self.spawn_scan() {
            self.rescan_pending.store(true, Ordering::SeqCst);
        }
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
            if state.rescan_pending.swap(false, Ordering::SeqCst) {
                state.spawn_scan();
            }
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
