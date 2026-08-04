//! Wikipedia summary fetch (es → en fallback) with on-disk caching, for the game
//! detail page. Ported from the old Tauri command; plain async, no UI deps.

use crate::store::Paths;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct WikiInfo {
    pub title: String,
    pub extract: String,
    pub url: Option<String>,
    pub thumbnail: Option<String>,
    pub lang: String,
}

fn cache_key(title: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in title.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

async fn fetch_lang(client: &reqwest::Client, lang: &str, title: &str) -> Option<WikiInfo> {
    let enc = urlencoding::encode(title);
    let url = format!("https://{lang}.wikipedia.org/api/rest_v1/page/summary/{enc}?redirect=true");
    let resp = client
        .get(&url)
        .header("User-Agent", "Freeport/0.2 (decompilation launcher)")
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: serde_json::Value = resp.json().await.ok()?;
    if v.get("type").and_then(|t| t.as_str()) == Some("disambiguation") {
        return None;
    }
    let extract = v.get("extract").and_then(|e| e.as_str())?.trim().to_string();
    if extract.is_empty() {
        return None;
    }
    Some(WikiInfo {
        title: v.get("title").and_then(|t| t.as_str()).unwrap_or(title).to_string(),
        extract,
        url: v
            .get("content_urls")
            .and_then(|c| c.get("desktop"))
            .and_then(|d| d.get("page"))
            .and_then(|p| p.as_str())
            .map(String::from),
        thumbnail: v
            .get("thumbnail")
            .and_then(|t| t.get("source"))
            .and_then(|s| s.as_str())
            .map(String::from),
        lang: lang.to_string(),
    })
}

/// Cached Wikipedia summary for `title` (Spanish preferred, English fallback).
pub async fn fetch(client: &reqwest::Client, paths: &Paths, title: &str) -> Option<WikiInfo> {
    let cache_dir = paths.data_dir.join("wiki_cache");
    let _ = std::fs::create_dir_all(&cache_dir);
    let cache_file = cache_dir.join(format!("{}.json", cache_key(title)));
    if let Ok(bytes) = std::fs::read(&cache_file) {
        if let Ok(info) = serde_json::from_slice::<WikiInfo>(&bytes) {
            return Some(info);
        }
    }
    let info = match fetch_lang(client, "es", title).await {
        Some(i) => Some(i),
        None => fetch_lang(client, "en", title).await,
    };
    if let Some(ref i) = info {
        if let Ok(bytes) = serde_json::to_vec(i) {
            let _ = std::fs::write(&cache_file, bytes);
        }
    }
    info
}
