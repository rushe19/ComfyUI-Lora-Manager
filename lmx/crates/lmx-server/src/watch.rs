//! Watch the model roots and rescan (debounced) when files appear, change or disappear.

use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use notify::{EventKind, RecursiveMode, Watcher};

use crate::{downloads::now, AppState};

/// Our own temporary files: their churn must not trigger scans.
fn ignored(p: &Path) -> bool {
    let s = p.to_string_lossy().to_ascii_lowercase();
    s.ends_with(".lmx.tmp") || s.ends_with(".lmx-part") || s.ends_with(".tmp") || s.contains("\\$recycle.bin") || s.contains("/$recycle.bin")
}

pub struct WatchHandle {
    _watcher: notify::RecommendedWatcher,
}

/// Start watching; quiet period 2 s, at most one scan per 5 s. Returns None when nothing to watch.
pub fn start(state: Arc<AppState>) -> Option<WatchHandle> {
    let dirty = Arc::new(AtomicBool::new(false));
    let last_event = Arc::new(AtomicU64::new(0));
    let (d, l) = (dirty.clone(), last_event.clone());
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        if matches!(ev.kind, EventKind::Access(_)) {
            return;
        }
        if ev.paths.iter().all(|p| ignored(p)) {
            return;
        }
        d.store(true, Ordering::SeqCst);
        l.store(now().to_bits(), Ordering::SeqCst);
    })
    .map_err(|e| tracing::warn!("file watcher unavailable: {e}"))
    .ok()?;
    let mut n = 0;
    for t in lmx_core::ModelType::ALL {
        for root in state.cfg.roots.get(t) {
            match watcher.watch(root, RecursiveMode::Recursive) {
                Ok(()) => n += 1,
                Err(e) => tracing::warn!("watch {}: {e}", root.display()),
            }
        }
    }
    if n == 0 {
        return None;
    }
    tracing::info!("watching {n} model folders for changes");
    tokio::spawn(async move {
        let mut last_scan = 0.0f64;
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if !dirty.load(Ordering::SeqCst) {
                continue;
            }
            let t = now();
            if t - f64::from_bits(last_event.load(Ordering::SeqCst)) < 2.0 || t - last_scan < 5.0 {
                continue;
            }
            dirty.store(false, Ordering::SeqCst);
            last_scan = t;
            tracing::debug!("change detected, rescanning");
            state.request_scan();
        }
    });
    Some(WatchHandle { _watcher: watcher })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_own_temp_files() {
        assert!(ignored(Path::new("H:/m/Lora/a.safetensors.lmx-part")));
        assert!(ignored(Path::new("H:/m/Lora/a.metadata.json.lmx.tmp")));
        assert!(ignored(Path::new("H:\\$RECYCLE.BIN\\S-1\\x")));
        assert!(!ignored(Path::new("H:/m/Lora/a.safetensors")));
        assert!(!ignored(Path::new("H:/m/Lora/a.metadata.json")));
    }
}
