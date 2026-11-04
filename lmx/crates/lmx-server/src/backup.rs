//! Settings backup: lmx's own settings as one JSON file (the model index is rebuilt by a
//! scan and needs no backup). `lmx.toml` is included for reference only, with any API key
//! redacted; restoring brings back prefs (search base models, auto check) and update ignores.

use std::sync::Arc;

use axum::{
    extract::State,
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{downloads::now, search_prefs::Prefs, AppState};

pub const FORMAT: &str = "lmx-settings";

/// Replace the value of any `*key*` / `password` / `token` / `secret` line in a TOML text,
/// commented-out lines included.
pub fn redact_toml(text: &str) -> String {
    text.lines()
        .map(|l| {
            let t = l.trim_start();
            let body = t.trim_start_matches('#').trim_start();
            let Some((k, _)) = body.split_once('=') else { return l.to_string() };
            let k = k.trim().to_ascii_lowercase();
            if k.contains("key") || k.contains("password") || k.contains("token") || k.contains("secret") {
                let prefix = &l[..l.len() - body.len()];
                format!("{prefix}{k} = \"[REDACTED]\"")
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub async fn export(State(st): State<Arc<AppState>>) -> Response {
    let toml = std::fs::read_to_string(st.cfg.data_dir.join("lmx.toml")).ok().map(|t| redact_toml(&t));
    let prefs = crate::search_prefs::load_prefs(&st);
    let (models, versions) = {
        let s = st.updates.stored.read().expect("updates");
        let mut m: Vec<i64> = s.ignored_models.iter().copied().collect();
        let mut v: Vec<i64> = s.ignored_versions.iter().copied().collect();
        m.sort_unstable();
        v.sort_unstable();
        (m, v)
    };
    let body = json!({
        "format": FORMAT,
        "version": 1,
        "created_at": now(),
        "lmx_version": env!("CARGO_PKG_VERSION"),
        "prefs": prefs,
        "updates_ignored": { "models": models, "versions": versions },
        "lmx_toml": toml,
    });
    let mut res = Json(body).into_response();
    res.headers_mut().insert(header::CONTENT_DISPOSITION, HeaderValue::from_static("attachment; filename=\"lmx-settings.json\""));
    res
}

#[derive(Deserialize)]
pub struct Ignored {
    #[serde(default)]
    pub models: Vec<i64>,
    #[serde(default)]
    pub versions: Vec<i64>,
}

#[derive(Deserialize)]
pub struct RestoreBody {
    pub format: String,
    pub prefs: Option<Prefs>,
    pub updates_ignored: Option<Ignored>,
}

pub async fn restore(State(st): State<Arc<AppState>>, Json(b): Json<Value>) -> Response {
    let b: RestoreBody = match serde_json::from_value(b) {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "format", "message": format!("lmx の設定ファイルではありません: {e}") }))).into_response(),
    };
    if b.format != FORMAT {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "format", "message": "lmx の設定ファイルではありません" }))).into_response();
    }
    let mut restored = Vec::new();
    if let Some(p) = b.prefs {
        if let Err(e) = crate::search_prefs::update_prefs(&st, |cur| *cur = p) {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "io", "message": e.to_string() }))).into_response();
        }
        restored.push("prefs");
    }
    if let Some(ig) = b.updates_ignored {
        crate::updates::set_ignored(&st, ig.models, ig.versions);
        restored.push("updates_ignored");
    }
    Json(json!({ "restored": restored })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_secrets_only() {
        let t = "lm_settings = \"x\"\ncivitai_api_key = \"abc123\"\n  # api_key = \"old-key-999\"\nscan_threads = 4";
        let r = redact_toml(t);
        assert!(!r.contains("abc123"));
        assert!(!r.contains("old-key-999"), "commented-out keys too");
        assert!(r.contains("  # api_key = \"[REDACTED]\""));
        assert!(r.contains("civitai_api_key = \"[REDACTED]\""));
        assert!(r.contains("lm_settings = \"x\""));
        assert!(r.contains("scan_threads = 4"));
    }
}
