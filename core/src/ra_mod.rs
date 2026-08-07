//! Install / enable / disable the Freeport RetroAchievements mod for a supported
//! recomp (currently the N64 recomps on the mod loader, e.g. Zelda64Recompiled).
//!
//! The mod artifacts (the `.nrm` and the native library) are embedded in the
//! binary and written into the recomp's own mods folder; the recomp's
//! `mods.json` is edited to enable/disable it. RetroAchievements credentials are
//! handed to the mod separately, via env vars set in `launch_project`.

use crate::error::{AppError, AppResult};
use crate::model::Project;
use serde_json::{json, Value};
use std::path::PathBuf;

/// The mod id, as declared in the mod manifest.
pub const MOD_ID: &str = "freeport_retroachievements";

// Embedded per-game mod packages (built from ~/freeport-ra-mod, staged under
// core/assets/ra). Each `.nrm` differs only in game_id + per-frame hook symbol;
// they all share the one native library below.
fn nrm_for(project_id: &str) -> Option<&'static [u8]> {
    Some(match project_id {
        "zelda64-recomp-mm" => include_bytes!("../assets/ra/zelda64-recomp-mm.nrm"),
        "banjo-recomp" => include_bytes!("../assets/ra/banjo-recomp.nrm"),
        "megaman64-recomp" => include_bytes!("../assets/ra/megaman64-recomp.nrm"),
        "bomberman64-recomp" => include_bytes!("../assets/ra/bomberman64-recomp.nrm"),
        "harvestmoon64-recomp" => include_bytes!("../assets/ra/harvestmoon64-recomp.nrm"),
        _ => return None,
    })
}

#[cfg(target_os = "linux")]
const NATIVE_BYTES: &[u8] = include_bytes!("../assets/ra/ra_native.so");
#[cfg(target_os = "linux")]
const NATIVE_NAME: &str = "ra_native.so";
#[cfg(target_os = "windows")]
const NATIVE_BYTES: &[u8] = include_bytes!("../assets/ra/ra_native.dll");
#[cfg(target_os = "windows")]
const NATIVE_NAME: &str = "ra_native.dll";

/// Whether the RA mod can be installed on this platform. The native library is
/// built for Linux (.so) and Windows (.dll).
pub fn platform_supported() -> bool {
    cfg!(any(target_os = "linux", target_os = "windows"))
}

/// The recomp's config directory (parent of `mods/` and `mods.json`), derived
/// from the game's launch binary name — same convention as the mod installer.
fn recomp_config_dir(project: &Project) -> AppResult<PathBuf> {
    let name = project
        .launch
        .get("linux")
        .and_then(|v| v.clone())
        .or_else(|| {
            project
                .launch
                .get("windows")
                .and_then(|v| v.clone())
                .map(|w| w.trim_end_matches(".exe").to_string())
        })
        .ok_or_else(|| AppError::msg("no se conoce la carpeta de configuración del juego"))?;
    let name = name.trim_end_matches(".exe");
    let cfg = dirs::config_dir().ok_or_else(|| AppError::msg("sin directorio de configuración"))?;
    Ok(cfg.join(name))
}

fn mods_json_path(project: &Project) -> AppResult<PathBuf> {
    Ok(recomp_config_dir(project)?.join("mods.json"))
}

/// The ROM the recomp stores in its own config dir (used to hash-identify the
/// game for RetroAchievements). Returns the largest .z64/.n64/.v64 there.
pub fn rom_path(project: &Project) -> Option<PathBuf> {
    let dir = recomp_config_dir(project).ok()?;
    let mut best: Option<(u64, PathBuf)> = None;
    for entry in std::fs::read_dir(&dir).ok()?.flatten() {
        let p = entry.path();
        let ext = p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
        if matches!(ext.as_deref(), Some("z64") | Some("n64") | Some("v64")) {
            let sz = p.metadata().map(|m| m.len()).unwrap_or(0);
            if best.as_ref().is_none_or(|(b, _)| sz > *b) {
                best = Some((sz, p));
            }
        }
    }
    best.map(|(_, p)| p)
}

fn read_mods_json(project: &Project) -> AppResult<Value> {
    let path = mods_json_path(project)?;
    match std::fs::read(&path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({ "enabled_mods": [], "mod_order": [] }))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(json!({ "enabled_mods": [], "mod_order": [] }))
        }
        Err(e) => Err(e.into()),
    }
}

fn write_mods_json(project: &Project, value: &Value) -> AppResult<()> {
    let path = mods_json_path(project)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// True if the RA mod is currently enabled in the recomp's mods.json.
pub fn is_enabled(project: &Project) -> bool {
    let Ok(value) = read_mods_json(project) else {
        return false;
    };
    value
        .get("enabled_mods")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(|m| m.as_str() == Some(MOD_ID)))
        .unwrap_or(false)
}

/// Install the mod files into the recomp's mods folder (idempotent).
#[cfg(any(target_os = "linux", target_os = "windows"))]
fn install_files(project: &Project) -> AppResult<()> {
    let nrm = nrm_for(&project.id)
        .ok_or_else(|| AppError::msg("este juego no tiene un mod de RetroAchievements"))?;
    let mods_dir = recomp_config_dir(project)?.join("mods");
    std::fs::create_dir_all(&mods_dir)?;
    std::fs::write(mods_dir.join(format!("{MOD_ID}.nrm")), nrm)?;
    std::fs::write(mods_dir.join(NATIVE_NAME), NATIVE_BYTES)?;
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn install_files(_project: &Project) -> AppResult<()> {
    Err(AppError::msg(
        "RetroAchievements no está disponible en esta plataforma",
    ))
}

fn add_to_array(value: &mut Value, key: &str, id: &str) {
    let arr = value
        .as_object_mut()
        .unwrap()
        .entry(key)
        .or_insert_with(|| json!([]));
    if let Some(arr) = arr.as_array_mut() {
        if !arr.iter().any(|m| m.as_str() == Some(id)) {
            arr.push(json!(id));
        }
    }
}

fn remove_from_array(value: &mut Value, key: &str, id: &str) {
    if let Some(arr) = value.get_mut(key).and_then(|v| v.as_array_mut()) {
        arr.retain(|m| m.as_str() != Some(id));
    }
}

/// Enable or disable RetroAchievements for a game. Enabling installs the mod
/// files and adds it to the recomp's enabled_mods; disabling only removes it
/// from enabled_mods (the files are left in place for a fast re-enable).
pub fn set_enabled(project: &Project, enable: bool) -> AppResult<()> {
    if enable && !platform_supported() {
        return Err(AppError::msg(
            "RetroAchievements no está disponible en esta plataforma",
        ));
    }
    let mut value = read_mods_json(project)?;
    if !value.is_object() {
        value = json!({ "enabled_mods": [], "mod_order": [] });
    }
    if enable {
        install_files(project)?;
        add_to_array(&mut value, "enabled_mods", MOD_ID);
        add_to_array(&mut value, "mod_order", MOD_ID);
    } else {
        remove_from_array(&mut value, "enabled_mods", MOD_ID);
        remove_from_array(&mut value, "mod_order", MOD_ID);
    }
    write_mods_json(project, &value)?;
    Ok(())
}
