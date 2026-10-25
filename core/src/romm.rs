//! RomM (rommapp) client: the user's own ROM library server. Freeport asks
//! it for the ROM a port needs (by system + title) and downloads it, instead
//! of the user hunting for the file on disk. Credentials live in the config
//! like the RetroAchievements ones; every request uses HTTP Basic auth.

use crate::error::{AppError, AppResult};
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct Romm {
    pub url: String,
    pub user: String,
    pub pass: String,
}

impl Romm {
    pub fn from_config(cfg: &crate::model::Config) -> Option<Romm> {
        let url = cfg.romm_url.as_deref()?.trim().trim_end_matches('/').to_string();
        if url.is_empty() {
            return None;
        }
        Some(Romm {
            url,
            user: cfg.romm_user.clone().unwrap_or_default(),
            pass: cfg.romm_pass.clone().unwrap_or_default(),
        })
    }

    fn get(&self, client: &reqwest::Client, path: &str) -> reqwest::RequestBuilder {
        client
            .get(format!("{}{}", self.url, path))
            .basic_auth(&self.user, Some(&self.pass))
            .header("User-Agent", "freeport")
    }

    /// Download URL with the credentials embedded (for the streaming helper).
    pub fn content_url(&self, rom: &Rom) -> String {
        let mut u = reqwest::Url::parse(&format!(
            "{}/api/roms/{}/content/{}",
            self.url,
            rom.id,
            urlencoding::encode(&rom.fs_name)
        ))
        .expect("romm url");
        let _ = u.set_username(&self.user);
        let _ = u.set_password(Some(&self.pass));
        u.to_string()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Platform {
    pub id: i64,
    pub slug: String,
    #[serde(default)]
    pub fs_slug: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub rom_count: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rom {
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub fs_name: String,
    #[serde(default)]
    pub fs_size_bytes: u64,
    #[serde(default)]
    pub platform_slug: String,
    #[serde(default)]
    pub platform_fs_slug: String,
}

#[derive(Deserialize)]
struct RomPage {
    items: Vec<Rom>,
}

/// RomM platform slugs (and folder slugs) that hold a Freeport system's games.
pub fn slugs_for_system(system: &str) -> &'static [&'static str] {
    match system {
        "n64" => &["n64"],
        "psx" => &["psx", "ps"],
        "ps2" => &["ps2"],
        "ps3" => &["ps3"],
        "ps4" => &["ps4"],
        "ps5" => &["ps5"],
        "psp" => &["psp"],
        "gc" => &["ngc", "gc", "gamecube"],
        "wii" => &["wii"],
        "wiiu" => &["wiiu"],
        "nds" => &["nds"],
        "3ds" => &["3ds", "n3ds"],
        "gb" => &["gb", "gbc"],
        "gba" => &["gba"],
        "dc" => &["dc", "dreamcast"],
        "xbox" => &["xbox"],
        "x360" => &["xbox360", "x360"],
        "switch" => &["switch"],
        "arcade" => &["arcade", "mame", "fbneo"],
        "pc" => &["win", "dos", "pc"],
        _ => &[],
    }
}

/// Server version and number of platforms with games: the "test connection".
pub async fn probe(client: &reqwest::Client, romm: &Romm) -> AppResult<(String, usize)> {
    #[derive(Deserialize)]
    struct Hb {
        #[serde(rename = "SYSTEM")]
        system: HbSys,
    }
    #[derive(Deserialize)]
    struct HbSys {
        #[serde(rename = "VERSION")]
        version: String,
    }
    let hb: Hb = romm
        .get(client, "/api/heartbeat")
        .send()
        .await
        .map_err(|e| AppError::msg(format!("no se pudo conectar con RomM: {e}")))?
        .error_for_status()
        .map_err(|e| AppError::msg(format!("RomM respondió con error: {e}")))?
        .json()
        .await
        .map_err(|e| AppError::msg(format!("respuesta inesperada de RomM: {e}")))?;
    let plats = platforms(client, romm).await?;
    Ok((hb.system.version, plats.iter().filter(|p| p.rom_count > 0).count()))
}

pub async fn platforms(client: &reqwest::Client, romm: &Romm) -> AppResult<Vec<Platform>> {
    let r = romm.get(client, "/api/platforms").send().await.map_err(|e| AppError::msg(format!("RomM: {e}")))?;
    if r.status() == reqwest::StatusCode::UNAUTHORIZED || r.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(AppError::msg("RomM rechazó el usuario o la contraseña"));
    }
    r.error_for_status()
        .map_err(|e| AppError::msg(format!("RomM: {e}")))?
        .json()
        .await
        .map_err(|e| AppError::msg(format!("RomM (plataformas): {e}")))
}

/// Games of the given platforms whose title matches `term`.
pub async fn search(client: &reqwest::Client, romm: &Romm, platform_ids: &[i64], term: &str) -> AppResult<Vec<Rom>> {
    let mut req = romm.get(client, "/api/roms").query(&[("search_term", term), ("limit", "30")]);
    for id in platform_ids {
        req = req.query(&[("platform_ids", id.to_string())]);
    }
    let page: RomPage = req
        .send()
        .await
        .map_err(|e| AppError::msg(format!("RomM: {e}")))?
        .error_for_status()
        .map_err(|e| AppError::msg(format!("RomM: {e}")))?
        .json()
        .await
        .map_err(|e| AppError::msg(format!("RomM (búsqueda): {e}")))?;
    Ok(page.items)
}

/// Lowercase alphanumerics only, so "The Legend of Zelda: Ocarina of Time"
/// and "Legend of Zelda, The - Ocarina of Time (USA).z64" compare equal-ish.
pub fn normalize(s: &str) -> String {
    // drop a short extension, then every "(USA)" / "[!]" style tag
    let s = match s.rsplit_once('.') {
        Some((a, ext)) if ext.len() <= 4 && !ext.contains(' ') => a,
        _ => s,
    };
    let mut cleaned = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            _ if depth == 0 => cleaned.push(c),
            _ => {}
        }
    }
    cleaned
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty() && !matches!(*w, "the" | "a" | "an"))
        .collect()
}

/// 0.0..1.0: how well a RomM entry matches the wanted title.
pub fn score(rom: &Rom, wanted: &str) -> f32 {
    let w = normalize(wanted);
    let n = normalize(&rom.name);
    let f = normalize(&rom.fs_name);
    let best = |c: &str| -> f32 {
        if c == w {
            1.0
        } else if c.starts_with(&w) || w.starts_with(c) {
            0.8
        } else if c.contains(&w) || w.contains(c) {
            0.6
        } else {
            // crude overlap of 3-grams
            fn grams(s: &str) -> Vec<&str> {
                (0..s.len().saturating_sub(2)).map(|i| &s[i..i + 3]).collect()
            }
            let (a, b) = (grams(&w), grams(c));
            if a.is_empty() || b.is_empty() {
                0.0
            } else {
                let hit = a.iter().filter(|g| b.contains(g)).count() as f32;
                0.5 * hit / a.len().max(b.len()) as f32
            }
        }
    };
    best(&n).max(best(&f))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_titles() {
        assert_eq!(normalize("The Legend of Zelda: Ocarina of Time"), normalize("Legend of Zelda, The - Ocarina of Time (USA).z64"));
        let r = Rom { id: 1, name: "Banjo-Kazooie".into(), fs_name: "Banjo-Kazooie (USA).z64".into(), fs_size_bytes: 0, platform_slug: "n64".into(), platform_fs_slug: "n64".into() };
        assert!(score(&r, "Banjo-Kazooie") > 0.9);
        assert!(score(&r, "Banjo-Tooie") < 0.9);
    }
}
