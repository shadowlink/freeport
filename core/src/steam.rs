//! "Add to Steam": writes a non-Steam-game shortcut into the newest Steam
//! user's `shortcuts.vdf`, launching the game through Freeport itself
//! (`freeport --play <id>`), so runner/RA/presence logic keeps applying.
//! Also drops the cover into Steam's grid folder as the portrait artwork.

use crate::error::{AppError, AppResult};
use std::path::{Path, PathBuf};
use steam_shortcuts_util::shortcut::ShortcutOwned;
use steam_shortcuts_util::{parse_shortcuts, shortcuts_to_bytes, Shortcut};

/// The `userdata/<id>/config` dir of the most recently used Steam user.
fn steam_user_config() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for base in [".local/share/Steam", ".steam/steam", ".steam/root"] {
        let userdata = home.join(base).join("userdata");
        let Ok(entries) = std::fs::read_dir(&userdata) else { continue };
        for e in entries.flatten() {
            let cfg = e.path().join("config");
            // Skip the anonymous account (id 0).
            if e.file_name().to_string_lossy() == "0" || !cfg.is_dir() {
                continue;
            }
            let stamp = std::fs::metadata(&cfg).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            if best.as_ref().map(|(t, _)| stamp > *t).unwrap_or(true) {
                best = Some((stamp, cfg));
            }
        }
    }
    best.map(|(_, p)| p)
}

/// The command Steam should run: the Freeport binary itself. When running from
/// an AppImage, `$APPIMAGE` is the persistent path (current_exe points into the
/// temporary mount).
fn freeport_exe() -> AppResult<PathBuf> {
    if let Some(p) = std::env::var_os("APPIMAGE") {
        return Ok(PathBuf::from(p));
    }
    Ok(std::env::current_exe()?)
}

/// Adds (or refreshes) a shortcut for `game_title`, launching `--play <id>`.
/// `cover` (optional) is copied as the grid portrait. Returns the app name
/// used. Steam picks the change up on its next restart.
pub fn add_shortcut(id: &str, game_title: &str, cover: Option<&Path>) -> AppResult<String> {
    let cfg = steam_user_config()
        .ok_or_else(|| AppError::msg("no se encontró un usuario de Steam en este equipo"))?;
    let vdf = cfg.join("shortcuts.vdf");
    let existing = std::fs::read(&vdf).unwrap_or_default();
    let mut shortcuts: Vec<ShortcutOwned> = if existing.is_empty() {
        Vec::new()
    } else {
        parse_shortcuts(&existing)
            .map_err(|e| AppError::msg(format!("shortcuts.vdf ilegible: {e}")))?
            .iter()
            .map(|s| s.to_owned())
            .collect()
    };

    let exe = freeport_exe()?;
    let exe_q = format!("\"{}\"", exe.display());
    let start_dir = exe.parent().map(|p| format!("\"{}\"", p.display())).unwrap_or_default();
    let launch = format!("--play {id}");

    // Refresh in place if we already added this game.
    shortcuts.retain(|s| !(s.app_name == game_title && s.launch_options == launch));
    let order = shortcuts.len().to_string();
    let mut sc = Shortcut::new(&order, game_title, &exe_q, &start_dir, "", "", &launch).to_owned();
    sc.tags = vec!["Freeport".to_string()];
    let app_id = sc.app_id;
    shortcuts.push(sc);

    let refs: Vec<Shortcut> = shortcuts.iter().map(|s| s.borrow()).collect();
    std::fs::write(&vdf, shortcuts_to_bytes(&refs))?;

    // Portrait grid art: grid/<appid>p.png (PNG re-encode of our cached cover).
    if let Some(cover) = cover {
        let grid = cfg.join("grid");
        let _ = std::fs::create_dir_all(&grid);
        if let Ok(img) = image::open(cover) {
            let _ = img.save_with_format(grid.join(format!("{app_id}p.png")), image::ImageFormat::Png);
        }
    }
    Ok(game_title.to_string())
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// Parses the real shortcuts.vdf on this machine (read-only sanity).
    #[test]
    #[ignore]
    fn parses_local_shortcuts_vdf() {
        let cfg = steam_user_config().expect("steam user");
        println!("steam config: {}", cfg.display());
        let vdf = cfg.join("shortcuts.vdf");
        if vdf.exists() {
            let bytes = std::fs::read(&vdf).unwrap();
            let n = parse_shortcuts(&bytes).expect("parse").len();
            println!("shortcuts existentes: {n}");
        } else {
            println!("sin shortcuts.vdf (se creará al añadir)");
        }
    }
}
