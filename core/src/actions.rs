//! High-level orchestration (install / launch / rom / uninstall / runners),
//! ported from the old Tauri `commands.rs` but UI-agnostic: plain functions over
//! the core modules. The native app calls these directly.

use crate::error::{AppError, AppResult};
use crate::model::{Config, InstalledEntry, Project};
use crate::store::{self, Paths};
use crate::{github, install, launch, platform, ra_mod};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_epoch() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

/// Official remote catalog (refreshed daily by CI). Overridable via config.
pub const DEFAULT_CATALOG_URL: &str =
    "https://raw.githubusercontent.com/shadowlink/freeport-catalog/main/catalog.json";

/// Fetches the remote catalog and caches it; returns the parsed catalog.
pub async fn refresh_catalog(
    client: &reqwest::Client,
    paths: &Paths,
    url: Option<&str>,
) -> AppResult<crate::model::Catalog> {
    let url = url.map(str::trim).filter(|s| !s.is_empty()).unwrap_or(DEFAULT_CATALOG_URL);
    let text = client
        .get(url)
        .header("User-Agent", "freeport")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let catalog: crate::model::Catalog = serde_json::from_str(&text)?;
    store::save_catalog_cache(paths, &catalog)?;
    Ok(catalog)
}

/// Fetches the latest release's changelog for a project: (tag, published_at, body).
///
/// Reads the public `releases.atom` feed first — it carries the notes and is
/// exempt from the 60 req/hour anonymous API limit, which browsing game pages
/// used to burn through (one API call per page). The API is only a fallback.
pub async fn fetch_changelog(
    client: &reqwest::Client,
    paths: &Paths,
    project: &Project,
) -> AppResult<(String, Option<String>, String)> {
    let slug = project.repo.slug();
    if let Ok(entries) = github::fetch_changelogs_noapi(client, &slug).await {
        // The catalog's probe already picked the channel's tag; fall back to
        // the newest entry when the cached tag isn't in the feed (or missing).
        let cached_tag = project.cached.as_ref().and_then(|c| c.latest_tag.as_deref());
        let entry = cached_tag
            .and_then(|t| entries.iter().find(|(tag, _, _)| tag == t))
            .or_else(|| entries.first());
        if let Some((tag, updated, body)) = entry {
            return Ok((tag.clone(), updated.clone(), body.clone()));
        }
    }
    let token = store::load_config(paths).ok().and_then(|c| c.github_token);
    let releases = github::fetch_releases(client, &slug, token.as_deref()).await?;
    let rel = github::pick_release(&releases, &project.release_channel, project.rolling_tag.as_deref())
        .ok_or_else(|| AppError::msg("sin release"))?;
    Ok((rel.tag_name.clone(), rel.published_at.clone(), rel.body.clone().unwrap_or_default()))
}

/// Detected Wine/Proton runner. `id` is understood by `launch_windows_with`.
#[derive(Clone)]
pub struct Runner {
    pub id: String,
    pub label: String,
    pub kind: String,
}

fn have_cmd(cmd: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(cmd).is_file()))
        .unwrap_or(false)
}

/// Wine/Proton runners available on this machine (Proton first — needs D3D12).
pub fn list_runners() -> Vec<Runner> {
    let mut runners = Vec::new();
    let wine = have_cmd("wine");
    let umu = have_cmd("umu-run");

    let mut seen = std::collections::BTreeSet::new();
    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        for base in [".local/share/Steam", ".steam/steam", ".steam/root"] {
            roots.push(home.join(base).join("steamapps/common"));
            roots.push(home.join(base).join("compatibilitytools.d"));
        }
    }
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for e in entries.flatten() {
            let path = e.path();
            if !path.is_dir() || !path.join("proton").is_file() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if !seen.insert(name.clone()) {
                continue;
            }
            let id = if umu {
                format!("umu:{}", path.display())
            } else {
                format!("proton:{}", path.display())
            };
            runners.push(Runner { id, label: format!("Proton: {name}"), kind: "proton".into() });
        }
    }
    if umu {
        runners.push(Runner {
            id: "umu:".into(),
            label: "Proton (umu, automático)".into(),
            kind: "proton".into(),
        });
    }
    if wine {
        runners.push(Runner {
            id: "wine".into(),
            label: "Wine (sistema — sin D3D12)".into(),
            kind: "wine".into(),
        });
    }
    runners
}

fn resolve_runner(cfg: &Config, id: &str) -> Option<String> {
    if let Some(r) = cfg.game_runners.get(id) {
        return Some(r.clone());
    }
    if let Some(r) = &cfg.wine_runner {
        return Some(r.clone());
    }
    list_runners().first().map(|r| r.id.clone())
}

/// Picks which platform's asset to install: (triple, runs-through-wine).
/// A rule alone is not enough — the catalog's probed `cached.platforms` must
/// confirm the release actually ships that binary (a rule for a platform the
/// project hasn't published yet would otherwise win and fail at asset-pick,
/// e.g. an .appimage rule while releases are still Windows-only).
fn choose_install_triple(project: &Project, triple: &str, show_windows: bool) -> Option<(String, bool)> {
    let shipped = |t: &str| match project.cached.as_ref() {
        Some(c) if !c.platforms.is_empty() => c.platforms.iter().any(|x| x == t),
        _ => true, // no probe data — trust the rule
    };
    if project.asset_rules.contains_key(triple) && shipped(triple) {
        return Some((triple.to_string(), false));
    }
    if !triple.starts_with("windows")
        && show_windows
        && project.asset_rules.contains_key("windows-x86_64")
        && shipped("windows-x86_64")
    {
        return Some(("windows-x86_64".to_string(), true));
    }
    None
}

/// Resolves the release to install. Tries the quota-free path first: the
/// catalog's daily probe already resolved the channel's tag (`cached.latest_tag`),
/// and GitHub's public asset pages list its downloads without touching
/// api.github.com — anonymous API calls are capped at 60/hour per IP, which is
/// what used to make installs fail with 403 after browsing a while. The REST
/// API remains as fallback (no cached tag, stale tag, or layout changes).
async fn resolve_release(
    client: &reqwest::Client,
    project: &Project,
    token: Option<&str>,
    rule: &str,
) -> AppResult<github::Release> {
    let slug = project.repo.slug();
    if let Some(cached) = project.cached.as_ref() {
        if let Some(tag) = cached.latest_tag.as_deref() {
            if let Ok(mut rel) = github::fetch_release_by_tag_noapi(client, &slug, tag).await {
                if github::pick_asset(&rel, rule).is_ok() {
                    // The scraped page has no date; the probe's does, and it is
                    // this very release — keeps date-based update detection alive.
                    rel.published_at = cached.published_at.clone();
                    return Ok(rel);
                }
            }
        }
    }
    let releases = github::fetch_releases(client, &slug, token).await?;
    github::pick_release(&releases, &project.release_channel, project.rolling_tag.as_deref())
        .ok_or_else(|| AppError::msg("no se encontró una release adecuada"))
}

/// Downloads + extracts the right release asset for `project` and records it.
///
/// Safe by construction: everything happens in a staging directory and the
/// existing install is only swapped out at the very end, so a failed or
/// cancelled download/extract never destroys what was already installed —
/// and updates carry over user files (ROMs, saves, wine prefix, mods).
/// `cancel`, when set, aborts mid-download at the next chunk.
pub async fn install_project(
    client: &reqwest::Client,
    paths: &Paths,
    project: &Project,
    cfg: &Config,
    cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    mut on_progress: impl FnMut(u64, u64),
) -> AppResult<InstalledEntry> {
    let triple = platform::current_triple();
    let token = cfg.github_token.as_deref();

    let (install_triple, is_windows_install) = choose_install_triple(project, &triple, cfg.show_windows)
        .ok_or_else(|| {
            AppError::msg(format!("{} no publica binario para {triple}", project.name))
        })?;
    let rule = project.asset_rules.get(&install_triple).unwrap();

    let release = resolve_release(client, project, token, rule).await?;
    let asset = github::pick_asset(&release, rule)?;

    let app_dir = paths.app_dir(&project.id);
    let staging = app_dir.with_file_name(format!(".{}.staging", project.id));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    std::fs::create_dir_all(&staging)?;

    let result = stage_install(client, &release, &asset, &staging, cancel, &mut on_progress).await;
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }

    let installed_before = store::load_installed(paths)?;
    if app_dir.exists() {
        // Update: carry the user's files over, then swap directories.
        let mut extras: Vec<std::path::PathBuf> = Vec::new();
        if let Some(rom) = installed_before.get(&project.id).and_then(|e| e.rom_path.as_ref()) {
            if let Ok(rel) = Path::new(rom).strip_prefix(&app_dir) {
                extras.push(rel.to_path_buf());
            }
        }
        install::preserve_user_files(&app_dir, &staging, &extras)?;
        let old = app_dir.with_file_name(format!(".{}.old", project.id));
        if old.exists() {
            std::fs::remove_dir_all(&old)?;
        }
        std::fs::rename(&app_dir, &old)?;
        if let Err(e) = std::fs::rename(&staging, &app_dir) {
            // Roll the old install back before failing.
            let _ = std::fs::rename(&old, &app_dir);
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e.into());
        }
        let _ = std::fs::remove_dir_all(&old);
    } else {
        std::fs::rename(&staging, &app_dir)?;
    }

    let mut installed = installed_before;
    let entry = InstalledEntry {
        installed_tag: Some(release.tag_name.clone()),
        published_at: release.published_at.clone(),
        install_path: app_dir.display().to_string(),
        rom_path: installed.get(&project.id).and_then(|e| e.rom_path.clone()),
        installed_at: Some(now_epoch()),
        windows: is_windows_install,
        last_played: installed.get(&project.id).and_then(|e| e.last_played.clone()),
        play_secs: installed.get(&project.id).map(|e| e.play_secs).unwrap_or(0),
    };
    installed.insert(project.id.clone(), entry.clone());
    store::save_installed(paths, &installed)?;
    Ok(entry)
}

/// Downloads, checksum-verifies (when the release publishes SHA256SUMS) and
/// extracts one asset into `staging`.
async fn stage_install(
    client: &reqwest::Client,
    release: &github::Release,
    asset: &github::Asset,
    staging: &Path,
    cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    on_progress: &mut impl FnMut(u64, u64),
) -> AppResult<()> {
    let archive = staging.join(&asset.name);
    install::download_to_file_cancellable(
        client,
        &asset.browser_download_url,
        &archive,
        cancel.clone(),
        |d, t| on_progress(d, t),
    )
    .await?;

    // Integrity: many releases ship a SHA256SUMS(.txt) next to the binaries.
    if let Some(sums) = release.assets.iter().find(|a| {
        let n = a.name.to_lowercase();
        n == "sha256sums" || n == "sha256sums.txt" || n == format!("{}.sha256", asset.name.to_lowercase())
    }) {
        let text = client
            .get(&sums.browser_download_url)
            .header("User-Agent", "decompdeck")
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let expected = text.lines().find_map(|l| {
            let mut it = l.split_whitespace();
            let hash = it.next()?;
            let file = it.next()?.trim_start_matches('*');
            (file == asset.name || file.ends_with(&format!("/{}", asset.name))).then(|| hash.to_lowercase())
        });
        if let Some(expected) = expected {
            let got = install::file_sha256(&archive)?;
            if got != expected {
                return Err(AppError::msg(format!(
                    "la descarga de {} no supera la verificación SHA-256 (esperado {}…, obtenido {}…)",
                    asset.name,
                    &expected[..12.min(expected.len())],
                    &got[..12]
                )));
            }
        }
    }

    let cancelled = || cancel.as_ref().map(|c| c.load(std::sync::atomic::Ordering::Relaxed)).unwrap_or(false);
    if cancelled() {
        return Err(AppError::Cancelled);
    }
    if install::is_archive(&asset.name) {
        install::extract_archive(archive.clone(), staging.to_path_buf()).await?;
        let _ = std::fs::remove_file(&archive);
    } else {
        install::make_executable(&archive);
    }
    if cancelled() {
        return Err(AppError::Cancelled);
    }
    Ok(())
}

/// Copies the user's ROM next to the launch binary so the port finds it.
pub fn set_rom(paths: &Paths, project: &Project, rom_source: &str) -> AppResult<()> {
    let mut installed = store::load_installed(paths)?;
    let entry = installed
        .get_mut(&project.id)
        .ok_or_else(|| AppError::msg("el proyecto no está instalado"))?;

    let source = Path::new(rom_source);
    if !source.exists() {
        return Err(AppError::msg("el archivo de ROM indicado no existe"));
    }
    let hint_os = if entry.windows { "windows" } else { std::env::consts::OS };
    let hint = project.launch.get(hint_os).and_then(|v| v.clone());
    let install_dir = Path::new(&entry.install_path);
    let bin = install::find_launch_binary(install_dir, hint.as_deref(), entry.windows)?;
    let mut target_dir = bin.parent().unwrap_or(install_dir).to_path_buf();
    if let Some(sub) = project.rom.subdir.as_ref() {
        target_dir = target_dir.join(sub);
        std::fs::create_dir_all(&target_dir)?;
    }
    let target_name = project
        .rom
        .expected_filename
        .clone()
        .or_else(|| source.file_name().and_then(|n| n.to_str()).map(|s| s.to_string()))
        .ok_or_else(|| AppError::msg("nombre de ROM inválido"))?;
    let target = target_dir.join(target_name);
    std::fs::copy(source, &target)?;
    entry.rom_path = Some(target.display().to_string());
    store::save_installed(paths, &installed)
}

/// Process-wide Discord Rich Presence (lazily connected, best-effort).
fn presence() -> &'static crate::discord::DiscordPresence {
    use std::sync::OnceLock;
    static P: OnceLock<crate::discord::DiscordPresence> = OnceLock::new();
    P.get_or_init(crate::discord::DiscordPresence::new)
}

/// Launches an installed game (native or via Wine/Proton). Returns the PID.
pub fn launch_project(paths: &Paths, project: &Project) -> AppResult<u32> {
    let installed = store::load_installed(paths)?;
    let entry = installed
        .get(&project.id)
        .ok_or_else(|| AppError::msg("el proyecto no está instalado"))?;
    let install_dir = Path::new(&entry.install_path);
    let hint_os = if entry.windows { "windows" } else { std::env::consts::OS };
    let hint = project.launch.get(hint_os).and_then(|v| v.clone());
    let bin = install::find_launch_binary(install_dir, hint.as_deref(), entry.windows)?;

    // Hand RetroAchievements credentials to the in-game RA mod via env (the mod,
    // when present, reads these; harmless otherwise). The child inherits our env.
    if let Ok(cfg) = store::load_config(paths) {
        if let (Some(u), Some(t)) = (cfg.ra_user.as_deref(), cfg.ra_token.as_deref()) {
            std::env::set_var("FREEPORT_RA_USER", u);
            std::env::set_var("FREEPORT_RA_TOKEN", t);
        }
    }
    // Tell the RA mod which ROM to hash-identify. Per-game (each recomp keeps its
    // ROM in its own config dir); without this the mod would identify the wrong
    // game. env is process-global, so clear it for non-RA games.
    match ra_mod::rom_path(project) {
        Some(rom) if project.ra_supported => std::env::set_var("FREEPORT_RA_ROM", rom),
        _ => std::env::remove_var("FREEPORT_RA_ROM"),
    }

    let mut child = if entry.windows {
        let cfg = store::load_config(paths)?;
        let runner = resolve_runner(&cfg, &project.id).ok_or_else(|| {
            AppError::msg("no hay ningún runner de Windows. Instala Wine o umu-launcher (Proton).")
        })?;
        let prefix = install_dir.join(".wineprefix");
        launch::launch_windows_with(&bin, &runner, &prefix)?
    } else {
        launch::launch_binary(&bin)?
    };
    let pid = child.id();

    // Record the launch timestamp now.
    {
        let mut installed = store::load_installed(paths)?;
        if let Some(e) = installed.get_mut(&project.id) {
            e.last_played = Some(now_epoch());
        }
        let _ = store::save_installed(paths, &installed);
    }

    // Discord Rich Presence: "Jugando <game>" (best-effort, off this thread).
    if let Some(app_id) = crate::discord::resolve_app_id(
        store::load_config(paths).ok().and_then(|c| c.discord_app_id).as_deref(),
    ) {
        let game = if project.original_game.is_empty() { project.name.clone() } else { project.original_game.clone() };
        let subtitle = project.name.clone();
        std::thread::spawn(move || presence().set_playing(&app_id, &game, &subtitle));
    }

    // Reap the child in the background (avoids a zombie), clear presence on exit,
    // and accumulate play time.
    let paths2 = paths.clone();
    let id2 = project.id.clone();
    let start = std::time::Instant::now();
    std::thread::spawn(move || {
        let _ = child.wait();
        presence().clear();
        let secs = start.elapsed().as_secs();
        if let Ok(mut installed) = store::load_installed(&paths2) {
            if let Some(e) = installed.get_mut(&id2) {
                e.play_secs = e.play_secs.saturating_add(secs);
                let _ = store::save_installed(&paths2, &installed);
            }
        }
    });
    Ok(pid)
}

pub fn uninstall_project(paths: &Paths, id: &str) -> AppResult<()> {
    let app_dir = paths.app_dir(id);
    if app_dir.exists() {
        std::fs::remove_dir_all(&app_dir)?;
    }
    let mut installed = store::load_installed(paths)?;
    installed.remove(id);
    store::save_installed(paths, &installed)
}

// ── Mods ────────────────────────────────────────────────────────────────

use crate::mods::{self, ModInfo};
use std::collections::HashMap;
use std::path::PathBuf;

/// Install directory of an installed project, if any.
pub fn installed_dir(paths: &Paths, id: &str) -> Option<PathBuf> {
    store::load_installed(paths).ok()?.get(id).map(|e| PathBuf::from(&e.install_path))
}

/// Where mods live: `<install>/mods` for GameBanana/SoH ports, else the port's
/// `~/.config/<AppName>/mods`.
fn mods_dir_for(project: &Project, source: &str, install_dir: Option<&Path>) -> AppResult<PathBuf> {
    if source == "gamebanana" {
        let dir = install_dir
            .ok_or_else(|| AppError::msg("instala el juego primero para poder añadir mods"))?;
        return Ok(dir.join("mods"));
    }
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
    Ok(cfg.join(name).join("mods"))
}

/// Available mods for a project (Thunderstore community or GameBanana game id).
pub async fn list_mods(client: &reqwest::Client, project: &Project) -> AppResult<Vec<ModInfo>> {
    let src = project
        .mods
        .as_ref()
        .ok_or_else(|| AppError::msg("este juego no tiene fuente de mods configurada"))?;
    match src.source.as_str() {
        "gamebanana" => mods::fetch_gb_mods(client, &src.community).await,
        _ => mods::fetch_mods(client, &src.community).await,
    }
}

/// Installs a mod (with dependencies) and records it in mod_state.
pub async fn install_mod(
    client: &reqwest::Client,
    paths: &Paths,
    project: &Project,
    all: &[ModInfo],
    full_name: &str,
    on_progress: impl FnMut(&str, usize, usize, u64, u64, &str),
) -> AppResult<Vec<String>> {
    let src = project
        .mods
        .as_ref()
        .ok_or_else(|| AppError::msg("sin fuente de mods"))?
        .clone();
    let dir = mods_dir_for(project, &src.source, installed_dir(paths, &project.id).as_deref())?;
    let files = if src.source == "gamebanana" {
        mods::install_gb_mod(client, full_name, &dir, on_progress).await?
    } else {
        mods::install_mod(client, all, full_name, &dir, on_progress).await?
    };
    let version = all.iter().find(|m| m.full_name == full_name).map(|m| m.version.clone()).unwrap_or_default();
    let mut mstate = store::load_mod_state(paths)?;
    mstate
        .entry(project.id.clone())
        .or_default()
        .insert(full_name.to_string(), store::InstalledMod { version, files: files.clone() });
    store::save_mod_state(paths, &mstate)?;
    Ok(files)
}

/// full_name -> installed version, for a game.
pub fn installed_mods(paths: &Paths, id: &str) -> HashMap<String, String> {
    store::load_mod_state(paths)
        .ok()
        .and_then(|m| m.get(id).map(|mm| mm.iter().map(|(k, v)| (k.clone(), v.version.clone())).collect()))
        .unwrap_or_default()
}

/// Removes a mod's files (keeping ones shared with other installed mods).
pub fn uninstall_mod(paths: &Paths, project: &Project, full_name: &str) -> AppResult<()> {
    let source = project.mods.as_ref().map(|m| m.source.clone()).unwrap_or_default();
    let dir = mods_dir_for(project, &source, installed_dir(paths, &project.id).as_deref())?;
    let mut mstate = store::load_mod_state(paths)?;
    let files = mstate
        .get(&project.id)
        .and_then(|m| m.get(full_name))
        .map(|im| im.files.clone())
        .unwrap_or_default();
    let keep: std::collections::BTreeSet<String> = mstate
        .get(&project.id)
        .map(|m| {
            m.iter()
                .filter(|(k, _)| k.as_str() != full_name)
                .flat_map(|(_, v)| v.files.iter().cloned())
                .collect()
        })
        .unwrap_or_default();
    for f in &files {
        if keep.contains(f) {
            continue;
        }
        if let Some(name) = Path::new(f).file_name() {
            let target = dir.join(name);
            if target.exists() {
                let _ = std::fs::remove_file(&target);
            }
        }
    }
    if let Some(m) = mstate.get_mut(&project.id) {
        m.remove(full_name);
    }
    store::save_mod_state(paths, &mstate)
}

#[cfg(test)]
mod choose_triple_tests {
    use super::*;
    use crate::model::{Cached, Project};

    fn proj(rules: &[(&str, &str)], platforms: &[&str]) -> Project {
        let mut p: Project = serde_json::from_value(serde_json::json!({
            "id": "test", "name": "Test", "original_game": "Test", "system": "n64",
            "type": "recompilation",
            "repo": {"host": "github", "owner": "o", "repo": "r"},
        }))
        .expect("proyecto mínimo");
        for (k, v) in rules {
            p.asset_rules.insert((*k).into(), (*v).into());
        }
        p.cached = Some(Cached {
            platforms: platforms.iter().map(|s| s.to_string()).collect(),
            latest_tag: Some("v1".into()),
            published_at: None,
        });
        p
    }

    #[test]
    fn speculative_native_rule_falls_back_to_windows() {
        // MPH case: linux rule exists but the release only ships Windows.
        let p = proj(
            &[("linux-x86_64", "appimage$"), ("windows-x86_64", "zip$")],
            &["windows-x86_64"],
        );
        assert_eq!(
            choose_install_triple(&p, "linux-x86_64", true),
            Some(("windows-x86_64".into(), true))
        );
        // Windows games hidden → nothing to install.
        assert_eq!(choose_install_triple(&p, "linux-x86_64", false), None);
    }

    #[test]
    fn native_wins_when_actually_shipped() {
        let p = proj(
            &[("linux-x86_64", "appimage$"), ("windows-x86_64", "zip$")],
            &["linux-x86_64", "windows-x86_64"],
        );
        assert_eq!(
            choose_install_triple(&p, "linux-x86_64", true),
            Some(("linux-x86_64".into(), false))
        );
    }

    #[test]
    fn no_probe_data_trusts_the_rule() {
        let mut p = proj(&[("linux-x86_64", "zip$")], &[]);
        p.cached = None;
        assert_eq!(
            choose_install_triple(&p, "linux-x86_64", false),
            Some(("linux-x86_64".into(), false))
        );
    }
}

#[cfg(test)]
mod live_install_tests {
    use super::*;

    /// Live e2e for the MPH bug: Windows-only game on Linux with show_windows,
    /// after removing the speculative linux rule. Installs for real.
    #[tokio::test]
    #[ignore]
    async fn installs_mph_via_proton_path() {
        let paths = Paths::resolve().unwrap();
        let catalog = store::load_catalog(&paths).unwrap();
        let p = catalog.projects.iter().find(|p| p.id == "mph-recomp").unwrap().clone();
        let mut cfg = store::load_config(&paths).unwrap_or_default();
        cfg.show_windows = true;
        let client = reqwest::Client::new();
        let entry = install_project(&client, &paths, &p, &cfg, None, |_, _| {}).await.expect("install");
        println!("instalado {} windows={} en {}", entry.installed_tag.unwrap_or_default(), entry.windows, entry.install_path);
        assert!(entry.windows);
        let bin = install::find_launch_binary(Path::new(&entry.install_path), p.launch.get("windows").and_then(|v| v.clone()).as_deref(), true).expect("exe");
        println!("exe: {}", bin.display());
    }
}
