//! Preview delivery: small previews are served as-is, large still images get a cached JPEG thumbnail.

use std::{
    io::BufWriter,
    path::{Path as FsPath, PathBuf},
    sync::Arc,
};

use axum::{
    body::Body,
    extract::{Path, Request, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use sha2::{Digest, Sha256};
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::AppState;

/// Previews at or below this size are sent untouched (LoRA Manager previews are ~24 KB WebP).
pub const PASSTHROUGH_BYTES: u64 = 160 * 1024;
pub const THUMB_MAX: u32 = 512;
const ALLOWED: &[&str] = &["webp", "png", "jpg", "jpeg", "gif", "mp4", "webm", "avif"];

fn allowed(p: &FsPath) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| ALLOWED.contains(&e.to_ascii_lowercase().as_str()))
}

fn needs_thumb(p: &FsPath, size: u64) -> bool {
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    size > PASSTHROUGH_BYTES && matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp")
}

pub fn thumb_path(dir: &FsPath, src: &FsPath, size: u64, mtime: f64) -> PathBuf {
    let mut h = Sha256::new();
    h.update(src.to_string_lossy().as_bytes());
    h.update(size.to_le_bytes());
    h.update(mtime.to_le_bytes());
    h.update(THUMB_MAX.to_le_bytes());
    dir.join(format!("{}.jpg", &hex::encode(h.finalize())[..32]))
}

/// Decode, downscale to fit THUMB_MAX, encode JPEG q82. Writes atomically (tmp + rename).
pub fn make_thumb(src: &FsPath, dst: &FsPath) -> anyhow::Result<()> {
    let img = image::ImageReader::open(src)?.with_guessed_format()?.decode()?;
    let t = img.thumbnail(THUMB_MAX, THUMB_MAX).to_rgb8();
    let tmp = dst.with_extension("tmp");
    {
        let f = std::fs::File::create(&tmp)?;
        let mut w = BufWriter::new(f);
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut w, 82).encode_image(&t)?;
    }
    std::fs::rename(&tmp, dst)?;
    Ok(())
}

async fn serve(path: PathBuf, req: Request) -> Response {
    match ServeFile::new(path).oneshot(req).await {
        Ok(mut res) => {
            if res.status().is_success() {
                // URLs carry ?v=<sidecar mtime>, so they can be cached hard.
                res.headers_mut()
                    .insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=604800, immutable"));
            }
            res.map(Body::new)
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn preview(State(st): State<Arc<AppState>>, Path(id): Path<i64>, req: Request) -> Response {
    let not_found = || (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "preview not found"}))).into_response();
    let Some(rec) = st.library().get(id) else { return not_found() };
    let Some(src) = rec.preview_path.as_deref().map(PathBuf::from) else { return not_found() };
    if !allowed(&src) {
        return not_found();
    }
    let Ok(md) = tokio::fs::metadata(&src).await else { return not_found() };
    if !needs_thumb(&src, md.len()) {
        return serve(src, req).await;
    }
    let mtime = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let dst = thumb_path(&st.thumbs_dir, &src, md.len(), mtime);
    if tokio::fs::metadata(&dst).await.is_err() {
        let _permit = st.thumb_permits.acquire().await;
        if tokio::fs::metadata(&dst).await.is_err() {
            let (s, d) = (src.clone(), dst.clone());
            match tokio::task::spawn_blocking(move || make_thumb(&s, &d)).await {
                Ok(Ok(())) => {}
                // Undecodable image: fall back to the original file.
                _ => return serve(src, req).await,
            }
        }
    }
    serve(dst, req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumb_rules() {
        assert!(!needs_thumb(FsPath::new("a.webp"), 24 * 1024));
        assert!(needs_thumb(FsPath::new("a.PNG"), 2 * 1024 * 1024));
        assert!(!needs_thumb(FsPath::new("a.mp4"), 20 * 1024 * 1024));
        assert!(!allowed(FsPath::new("C:/Windows/win.ini")));
        assert!(allowed(FsPath::new("x.preview.MP4")));
    }

    #[test]
    fn make_thumb_downscales_png() {
        let dir = std::env::temp_dir().join(format!("lmx-thumb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("big.png");
        image::RgbImage::from_fn(1200, 1600, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])).save(&src).unwrap();
        let dst = dir.join("t.jpg");
        make_thumb(&src, &dst).unwrap();
        let t = image::open(&dst).unwrap();
        assert_eq!((t.width(), t.height()), (384, 512));
    }
}
