//! Cover thumbnail cache. Downloads a cover once, downscales it to ~320px JPEG,
//! caches it on disk, and serves it to the webview via the `cover://` URI scheme.
//! Smaller images = less decode/paint work while scrolling the grid.

use crate::store::Paths;
use std::path::PathBuf;

/// Stable FNV-1a hex of a URL (used for cache filenames).
fn hash_hex(url: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in url.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn cache_key(url: &str) -> String {
    format!("{}.jpg", hash_hex(url))
}

fn cache_path(paths: &Paths, url: &str) -> PathBuf {
    paths.cover_cache_dir().join(cache_key(url))
}

/// Libretro in-game snap + title-screen URLs derived from a boxart URL.
pub fn screenshot_urls(cover_url: &str) -> Vec<String> {
    if !cover_url.contains("/Named_Boxarts/") {
        return Vec::new();
    }
    vec![
        cover_url.replace("/Named_Boxarts/", "/Named_Snaps/"),
        cover_url.replace("/Named_Boxarts/", "/Named_Titles/"),
    ]
}

/// Downloads a full-size image (e.g. a screenshot) to `screens_cache`, returning
/// its local path. Used for the detail-page gallery (opened in the OS viewer).
pub async fn get_full(client: &reqwest::Client, paths: &Paths, url: &str) -> Result<PathBuf, String> {
    let dir = paths.data_dir.join("screens_cache");
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join(format!("{}.png", hash_hex(url)));
    if file.exists() {
        return Ok(file);
    }
    let resp = client
        .get(url)
        .header("User-Agent", "freeport")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    std::fs::write(&file, &bytes).map_err(|e| e.to_string())?;
    Ok(file)
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

/// Absolute path of the (possibly not-yet-generated) thumbnail for a cover URL.
pub fn path_for(paths: &Paths, url: &str) -> std::path::PathBuf {
    cache_path(paths, url)
}
