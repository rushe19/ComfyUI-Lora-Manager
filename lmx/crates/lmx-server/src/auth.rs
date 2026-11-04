//! Optional password for the whole API (off by default).
//!
//! `data/auth.json` holds a PBKDF2-HMAC-SHA256 hash of the password and a random session
//! secret. Logging in sets an HttpOnly cookie `lmx_session=<issued>.<hmac(secret, issued)>`
//! that is valid for 180 days; changing the password rotates the secret (logs everyone out).
//! The PWA shell itself is served without a login so the login screen can load; every
//! `/api/*` route except health/auth needs the cookie. Forgot the password: delete auth.json.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{downloads::now, AppState};

const COOKIE: &str = "lmx_session";
const SESSION_DAYS: f64 = 180.0;
const ITERATIONS: u32 = 120_000;
const MAX_FAILS: u32 = 10;
const FAIL_WINDOW_SECS: f64 = 600.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthFile {
    pub salt: String,
    pub hash: String,
    pub iterations: u32,
    pub secret: String,
}

#[derive(Default)]
pub struct Auth {
    pub file: RwLock<Option<AuthFile>>,
    fails: Mutex<(u32, f64)>,
}

fn path(st: &AppState) -> PathBuf {
    st.cfg.data_dir.join("auth.json")
}

impl Auth {
    pub fn load(file: &std::path::Path) -> Auth {
        let f = std::fs::read(file).ok().and_then(|b| serde_json::from_slice(&b).ok());
        Auth { file: RwLock::new(f), ..Default::default() }
    }
    pub fn enabled(&self) -> bool {
        self.file.read().expect("auth").is_some()
    }
}

// ---- crypto helpers (HMAC-SHA256 / PBKDF2 per RFC 2104 / RFC 8018) ----

pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let inner = Sha256::new().chain_update(ipad).chain_update(msg).finalize();
    Sha256::new().chain_update(opad).chain_update(inner).finalize().into()
}

/// PBKDF2-HMAC-SHA256 with a single 32-byte block.
pub fn pbkdf2(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    let mut msg = salt.to_vec();
    msg.extend_from_slice(&1u32.to_be_bytes());
    let mut u = hmac_sha256(password, &msg);
    let mut out = u;
    for _ in 1..iterations {
        u = hmac_sha256(password, &u);
        for (o, x) in out.iter_mut().zip(u.iter()) {
            *o ^= x;
        }
    }
    out
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn random_hex(n: usize) -> std::io::Result<String> {
    let mut b = vec![0u8; n];
    getrandom::getrandom(&mut b).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(hex::encode(b))
}

pub fn make_file(password: &str) -> std::io::Result<AuthFile> {
    let salt = random_hex(16)?;
    let hash = hex::encode(pbkdf2(password.as_bytes(), salt.as_bytes(), ITERATIONS));
    Ok(AuthFile { salt, hash, iterations: ITERATIONS, secret: random_hex(32)? })
}

pub fn check_password(f: &AuthFile, password: &str) -> bool {
    let got = pbkdf2(password.as_bytes(), f.salt.as_bytes(), f.iterations);
    hex::decode(&f.hash).is_ok_and(|want| ct_eq(&got, &want))
}

pub fn make_token(f: &AuthFile, issued: u64) -> String {
    let mac = hmac_sha256(f.secret.as_bytes(), issued.to_string().as_bytes());
    format!("{issued}.{}", hex::encode(mac))
}

pub fn token_valid(f: &AuthFile, token: &str, at: f64) -> bool {
    let Some((issued, mac)) = token.split_once('.') else { return false };
    let Ok(t) = issued.parse::<u64>() else { return false };
    if (t as f64) > at + 60.0 || at - t as f64 > SESSION_DAYS * 86400.0 {
        return false;
    }
    let want = hmac_sha256(f.secret.as_bytes(), issued.as_bytes());
    hex::decode(mac).is_ok_and(|m| ct_eq(&m, &want))
}

fn cookie_token(h: &HeaderMap) -> Option<String> {
    h.get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == COOKIE)
        .map(|(_, v)| v.to_string())
}

fn logged_in(st: &AppState, h: &HeaderMap) -> bool {
    match &*st.auth.file.read().expect("auth") {
        None => true,
        Some(f) => cookie_token(h).is_some_and(|t| token_valid(f, &t, now())),
    }
}

fn save(st: &AppState, f: Option<&AuthFile>) -> std::io::Result<()> {
    let p = path(st);
    match f {
        Some(f) => {
            let tmp = p.with_extension("json.tmp");
            std::fs::write(&tmp, serde_json::to_vec_pretty(f).map_err(std::io::Error::other)?)?;
            std::fs::rename(&tmp, &p)?;
        }
        None => {
            if p.exists() {
                std::fs::remove_file(&p)?;
            }
        }
    }
    *st.auth.file.write().expect("auth") = f.cloned();
    Ok(())
}

fn session_cookie(value: &str, max_age: i64) -> HeaderValue {
    HeaderValue::from_str(&format!("{COOKIE}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}")).expect("cookie")
}

/// Middleware for `/api/*`: 401 `{error: "auth"}` unless the session cookie is valid.
pub async fn require(State(st): State<Arc<AppState>>, req: Request, next: Next) -> Response {
    let p = req.uri().path();
    let open = matches!(p, "/health" | "/auth/status" | "/auth/login" | "/auth/logout" | "/api/health" | "/api/auth/status" | "/api/auth/login" | "/api/auth/logout");
    if open || logged_in(&st, req.headers()) {
        return next.run(req).await;
    }
    (StatusCode::UNAUTHORIZED, [("x-lmx-auth", "required")], Json(json!({ "error": "auth", "message": "パスワードが必要です" }))).into_response()
}

pub async fn status(State(st): State<Arc<AppState>>, h: HeaderMap) -> Json<serde_json::Value> {
    Json(json!({ "enabled": st.auth.enabled(), "logged_in": logged_in(&st, &h) }))
}

#[derive(Deserialize)]
pub struct LoginBody {
    pub password: String,
}

pub async fn login(State(st): State<Arc<AppState>>, Json(b): Json<LoginBody>) -> Response {
    let Some(f) = st.auth.file.read().expect("auth").clone() else {
        return Json(json!({ "ok": true, "enabled": false })).into_response();
    };
    {
        let mut fl = st.auth.fails.lock().expect("fails");
        if now() - fl.1 > FAIL_WINDOW_SECS {
            *fl = (0, now());
        }
        if fl.0 >= MAX_FAILS {
            return (StatusCode::TOO_MANY_REQUESTS, Json(json!({ "error": "locked", "message": "失敗が続いたため 10 分間ログインできません" }))).into_response();
        }
    }
    let pw = b.password.clone();
    let f2 = f.clone();
    let ok = tokio::task::spawn_blocking(move || check_password(&f2, &pw)).await.unwrap_or(false);
    if !ok {
        st.auth.fails.lock().expect("fails").0 += 1;
        tokio::time::sleep(Duration::from_millis(600)).await;
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "password", "message": "パスワードが違います" }))).into_response();
    }
    *st.auth.fails.lock().expect("fails") = (0, now());
    let token = make_token(&f, now() as u64);
    let mut res = Json(json!({ "ok": true })).into_response();
    res.headers_mut().insert(header::SET_COOKIE, session_cookie(&token, (SESSION_DAYS * 86400.0) as i64));
    res
}

pub async fn logout() -> Response {
    let mut res = Json(json!({ "ok": true })).into_response();
    res.headers_mut().insert(header::SET_COOKIE, session_cookie("", 0));
    res
}

#[derive(Deserialize)]
pub struct PasswordBody {
    /// Required when a password is already set.
    pub current: Option<String>,
    /// New password; empty or null turns the password off.
    pub new: Option<String>,
}

/// Set, change or remove the password. Runs behind `require`, so the caller is logged in.
pub async fn set_password(State(st): State<Arc<AppState>>, Json(b): Json<PasswordBody>) -> Response {
    let existing = st.auth.file.read().expect("auth").clone();
    if let Some(f) = &existing {
        let cur = b.current.clone().unwrap_or_default();
        let f2 = f.clone();
        if !tokio::task::spawn_blocking(move || check_password(&f2, &cur)).await.unwrap_or(false) {
            tokio::time::sleep(Duration::from_millis(600)).await;
            return (StatusCode::FORBIDDEN, Json(json!({ "error": "password", "message": "今のパスワードが違います" }))).into_response();
        }
    }
    let new = b.new.unwrap_or_default();
    if new.is_empty() {
        if let Err(e) = save(&st, None) {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "io", "message": e.to_string() }))).into_response();
        }
        return Json(json!({ "enabled": false })).into_response();
    }
    if new.chars().count() < 6 {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "short", "message": "6 文字以上にしてください" }))).into_response();
    }
    let f = match tokio::task::spawn_blocking(move || make_file(&new)).await {
        Ok(Ok(f)) => f,
        Ok(Err(e)) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "io", "message": e.to_string() }))).into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "io", "message": e.to_string() }))).into_response(),
    };
    if let Err(e) = save(&st, Some(&f)) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "io", "message": e.to_string() }))).into_response();
    }
    // Keep the caller logged in with a session for the new secret.
    let mut res = Json(json!({ "enabled": true })).into_response();
    res.headers_mut().insert(header::SET_COOKIE, session_cookie(&make_token(&f, now() as u64), (SESSION_DAYS * 86400.0) as i64));
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_rfc4231_case_2() {
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(hex::encode(mac), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
    }

    #[test]
    fn pbkdf2_known_vectors() {
        // RFC 7914 section 11 (PBKDF2-HMAC-SHA256), first 32 bytes.
        assert_eq!(hex::encode(pbkdf2(b"passwd", b"salt", 1)), "55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc");
        assert_eq!(hex::encode(pbkdf2(b"password", b"salt", 4096)), "c5e478d59288c841aa530db6845c4c8d962893a001ce4e11a4963873aa98134a");
    }

    #[test]
    fn tokens() {
        let f = AuthFile { salt: "s".into(), hash: String::new(), iterations: 1, secret: "k".into() };
        let t = make_token(&f, 1_000_000);
        assert!(token_valid(&f, &t, 1_000_100.0));
        assert!(!token_valid(&f, &t, 1_000_000.0 + 181.0 * 86400.0), "expired");
        assert!(!token_valid(&f, "1000000.00", 1_000_100.0));
        let other = AuthFile { secret: "other".into(), ..f.clone() };
        assert!(!token_valid(&other, &t, 1_000_100.0), "rotated secret invalidates sessions");
    }
}
