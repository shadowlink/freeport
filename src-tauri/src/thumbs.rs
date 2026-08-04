//! Cover thumbnail cache. Downloads a cover once, downscales it to ~320px JPEG,
//! caches it on disk, and serves it to the webview via the `cover://` URI scheme.
//! Smaller images = less decode/paint work while scrolling the grid.

use crate::store::Paths;
use std::path::PathBuf;

/// Stable cache filename from the source URL (FNV-1a → hex, `.jpg`).
fn cache_key(url: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in url.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}.jpg")
}

fn cache_path(paths: &Paths, url: &str) -> PathBuf {
    paths.cover_cache_dir().join(cache_key(url))
}

/// Decode arbitrary image bytes and re-encode a ~320px-wide JPEG (aspect kept).
fn to_thumbnail(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    // thumbnail() preserves aspect ratio, fitting within the bounds.
    let thumb = img.thumbnail(320, 480);
    let rgb = image::DynamicImage::ImageRgb8(thumb.to_rgb8());
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 82)
        .encode_image(&rgb)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// Returns cached thumbnail bytes for `url`, generating (download + resize) on a
/// cache miss. Best-effort: any failure returns Err so the `<img>` falls back.
pub async fn get(client: &reqwest::Client, paths: &Paths, url: &str) -> Result<Vec<u8>, String> {
    let file = cache_path(paths, url);
    if let Ok(bytes) = std::fs::read(&file) {
        if !bytes.is_empty() {
            return Ok(bytes);
        }
    }
    let resp = client
        .get(url)
        .header("User-Agent", "freeport")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let orig = resp.bytes().await.map_err(|e| e.to_string())?;
    let jpeg = tokio::task::spawn_blocking(move || to_thumbnail(&orig))
        .await
        .map_err(|e| e.to_string())??;
    let _ = std::fs::write(&file, &jpeg);
    Ok(jpeg)
}

/// Extracts the `?src=<url>` parameter from a `cover://` request URI.
pub fn src_of(uri: &tauri::http::Uri) -> Option<String> {
    let q = uri.query()?;
    for pair in q.split('&') {
        if let Some(v) = pair.strip_prefix("src=") {
            return urlencoding::decode(v).ok().map(|s| s.into_owned());
        }
    }
    None
}
