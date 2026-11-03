//! HTTP client for the Civitai v1 API. The API key never leaves the server.

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use serde_json::Value;

use crate::limiter::Limiter;

const CACHE_TTL: Duration = Duration::from_secs(90);
const CACHE_MAX: usize = 200;

#[derive(Debug, Clone, Default)]
pub struct SearchParams {
    pub query: String,
    /// Civitai model types, e.g. LORA, LoCon, DoRA, Checkpoint, TextualInversion.
    pub types: Vec<String>,
    pub base_models: Vec<String>,
    /// "Highest Rated" | "Most Downloaded" | "Newest"
    pub sort: Option<String>,
    /// "AllTime" | "Year" | "Month" | "Week" | "Day"
    pub period: Option<String>,
    pub nsfw: bool,
    pub cursor: Option<String>,
    pub limit: u32,
    /// Only the authenticated user's favorites (requires an API key).
    pub favorites: bool,
    pub username: Option<String>,
}

impl SearchParams {
    pub fn to_query(&self) -> Vec<(String, String)> {
        let mut q: Vec<(String, String)> = Vec::new();
        let limit = if self.limit == 0 { 40 } else { self.limit.min(100) };
        q.push(("limit".into(), limit.to_string()));
        if !self.query.trim().is_empty() {
            q.push(("query".into(), self.query.trim().to_string()));
        }
        for t in &self.types {
            q.push(("types".into(), t.clone()));
        }
        for b in &self.base_models {
            q.push(("baseModels".into(), b.clone()));
        }
        if let Some(s) = &self.sort {
            q.push(("sort".into(), s.clone()));
        }
        if let Some(p) = &self.period {
            q.push(("period".into(), p.clone()));
        }
        q.push(("nsfw".into(), self.nsfw.to_string()));
        if let Some(c) = &self.cursor {
            q.push(("cursor".into(), c.clone()));
        }
        if self.favorites {
            q.push(("favorites".into(), "true".into()));
        }
        if let Some(u) = &self.username {
            q.push(("username".into(), u.clone()));
        }
        q
    }
}

#[derive(Debug)]
pub enum CivitaiError {
    Http(u16, String),
    Network(String),
    Decode(String),
    NeedsKey,
}

impl std::fmt::Display for CivitaiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CivitaiError::Http(code, body) => write!(f, "Civitai HTTP {code}: {body}"),
            CivitaiError::Network(e) => write!(f, "Civitai network error: {e}"),
            CivitaiError::Decode(e) => write!(f, "Civitai response error: {e}"),
            CivitaiError::NeedsKey => write!(f, "Civitai API key is required"),
        }
    }
}

impl std::error::Error for CivitaiError {}

pub struct CivitaiClient {
    http: reqwest::Client,
    api_base: String,
    key: Option<String>,
    limiter: Limiter,
    cache: Mutex<HashMap<String, (Instant, Value)>>,
}

impl CivitaiClient {
    pub fn new(host: &str, key: Option<String>) -> anyhow::Result<CivitaiClient> {
        Self::with_api_base(&format!("https://{host}/api/v1"), key)
    }

    pub fn with_api_base(api_base: &str, key: Option<String>) -> anyhow::Result<CivitaiClient> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("lmx/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .build()?;
        Ok(CivitaiClient {
            http,
            api_base: api_base.trim_end_matches('/').to_string(),
            key: key.filter(|k| !k.trim().is_empty()),
            limiter: Limiter::new(Duration::from_millis(250)),
            cache: Mutex::new(HashMap::new()),
        })
    }

    pub fn has_key(&self) -> bool {
        self.key.is_some()
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    fn cached(&self, url: &str) -> Option<Value> {
        let c = self.cache.lock().expect("cache lock");
        c.get(url).filter(|(t, _)| t.elapsed() < CACHE_TTL).map(|(_, v)| v.clone())
    }

    fn store(&self, url: String, v: &Value) {
        let mut c = self.cache.lock().expect("cache lock");
        if c.len() >= CACHE_MAX {
            c.retain(|_, (t, _)| t.elapsed() < CACHE_TTL);
            if c.len() >= CACHE_MAX {
                c.clear();
            }
        }
        c.insert(url, (Instant::now(), v.clone()));
    }

    async fn get_json(&self, path: &str, query: &[(String, String)], auth: bool) -> Result<Value, CivitaiError> {
        let url = reqwest::Url::parse_with_params(&format!("{}{}", self.api_base, path), query)
            .map_err(|e| CivitaiError::Decode(e.to_string()))?;
        let cache_key = format!("{}|{}", auth, url);
        if let Some(v) = self.cached(&cache_key) {
            return Ok(v);
        }
        self.limiter.wait().await;
        let mut req = self.http.get(url).timeout(Duration::from_secs(25)).header("Accept", "application/json");
        if auth {
            let Some(key) = &self.key else { return Err(CivitaiError::NeedsKey) };
            req = req.bearer_auth(key);
        } else if let Some(key) = &self.key {
            // Authenticated requests also see the user's own NSFW visibility settings.
            req = req.bearer_auth(key);
        }
        let res = req.send().await.map_err(|e| CivitaiError::Network(e.to_string()))?;
        let status = res.status();
        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(CivitaiError::Http(status.as_u16(), body.chars().take(200).collect()));
        }
        let v: Value = res.json().await.map_err(|e| CivitaiError::Decode(e.to_string()))?;
        self.store(cache_key, &v);
        Ok(v)
    }

    pub async fn search(&self, p: &SearchParams) -> Result<Value, CivitaiError> {
        self.get_json("/models", &p.to_query(), p.favorites).await
    }

    pub async fn model(&self, id: i64) -> Result<Value, CivitaiError> {
        self.get_json(&format!("/models/{id}"), &[], false).await
    }

    pub async fn version(&self, id: i64) -> Result<Value, CivitaiError> {
        self.get_json(&format!("/model-versions/{id}"), &[], false).await
    }

    /// Version for a file hash (`/model-versions/by-hash/{sha256}`); 404 when Civitai doesn't know it.
    pub async fn by_hash(&self, sha256: &str) -> Result<Value, CivitaiError> {
        self.get_json(&format!("/model-versions/by-hash/{}", sha256.to_ascii_lowercase()), &[], false).await
    }

    /// Start a file download; reqwest drops the Authorization header on the cross-host redirect
    /// to the signed storage URL.
    pub async fn download(&self, url: &str, from_byte: u64) -> Result<reqwest::Response, CivitaiError> {
        let mut req = self.http.get(url).header("Accept", "*/*");
        if let Some(key) = &self.key {
            req = req.bearer_auth(key);
        }
        if from_byte > 0 {
            req = req.header("Range", format!("bytes={from_byte}-"));
        }
        let res = req.send().await.map_err(|e| CivitaiError::Network(e.to_string()))?;
        let status = res.status();
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(if self.key.is_none() { CivitaiError::NeedsKey } else { CivitaiError::Http(status.as_u16(), "access denied".into()) });
        }
        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(CivitaiError::Http(status.as_u16(), body.chars().take(200).collect()));
        }
        Ok(res)
    }

    /// Fetch a small file (preview image) fully.
    pub async fn fetch_bytes(&self, url: &str) -> Result<Vec<u8>, CivitaiError> {
        let res = self.http.get(url).timeout(Duration::from_secs(30)).send().await.map_err(|e| CivitaiError::Network(e.to_string()))?;
        if !res.status().is_success() {
            return Err(CivitaiError::Http(res.status().as_u16(), String::new()));
        }
        Ok(res.bytes().await.map_err(|e| CivitaiError::Network(e.to_string()))?.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_query_encodes_repeated_params_and_defaults() {
        let p = SearchParams {
            query: " miku ".into(),
            types: vec!["LORA".into(), "LoCon".into()],
            base_models: vec!["Illustrious".into()],
            sort: Some("Most Downloaded".into()),
            nsfw: true,
            ..Default::default()
        };
        let q = p.to_query();
        assert!(q.contains(&("limit".into(), "40".into())));
        assert!(q.contains(&("query".into(), "miku".into())));
        assert_eq!(q.iter().filter(|(k, _)| k == "types").count(), 2);
        assert!(q.contains(&("nsfw".into(), "true".into())));
        assert!(!q.iter().any(|(k, _)| k == "favorites"));
    }
}
