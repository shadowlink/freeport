use crate::error::{AppError, AppResult};
use crate::model::*;
use crate::store::{self, Paths};
use crate::{github, install, launch, platform};
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

/// Shared, cheaply-cloneable application state.
pub struct AppState {
    pub client: reqwest::Client,
    pub paths: Paths,
    /// Per-community mod list cache (Thunderstore), so installs don't re-fetch
    /// the whole package list every time.
    pub mods_cache: std::sync::Mutex<std::collections::HashMap<String, Vec<crate::mods::ModInfo>>>,
    /// Discord Rich Presence connection (shows the running game as status).
    pub discord: crate::discord::DiscordPresence,
}

fn now_epoch() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

/// True if the project ships an asset for the given platform triple, using the
/// CI-probed `cached.platforms` when available and falling back to the presence
/// of an `asset_rules` entry.
fn supports_platform(p: &Project, triple: &str) -> bool {
    match &p.cached {
        Some(c) if !c.platforms.is_empty() => c.platforms.iter().any(|x| x == triple),
        _ => p.asset_rules.contains_key(triple),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> Catalog {
        serde_json::from_str(include_str!("../catalog.seed.json")).expect("seed parses")
    }

    #[test]
    fn seed_is_wellformed() {
        let cat = seed();
        assert!(cat.projects.len() >= 4, "seed should have several projects");
        for p in &cat.projects {
            assert!(!p.id.is_empty() && !p.name.is_empty());
            assert!(!p.repo.owner.is_empty() && !p.repo.repo.is_empty());
            // Every seeded project must ship at least one platform asset rule.
            // (Most are Linux-first, but Windows-only games run via Wine/Proton.)
            assert!(
                !p.asset_rules.is_empty(),
                "{} has no asset rules for any platform",
                p.id
            );
        }
    }

    #[test]
    fn platform_filter_hides_unsupported() {
        let cat = seed();
        let p = &cat.projects[0];
        assert!(supports_platform(p, "linux-x86_64"));
        // A platform no seed project targets must be filtered out.
        assert!(!supports_platform(p, "solaris-sparc"));
    }
}

// ---------------------------------------------------------------------------
// Basic info
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_platform() -> String {
    platform::current_triple()
}

/// A short encyclopedic summary for the game page, fetched from Wikipedia and
/// cached locally. Text is © its authors under CC BY-SA; the UI shows
/// attribution and a link back to the article.
#[derive(Serialize, serde::Deserialize, Clone)]
pub struct WikiInfo {
    pub title: String,
    pub extract: String,
    pub url: Option<String>,
    pub thumbnail: Option<String>,
    pub lang: String,
}

fn wiki_cache_key(title: &str) -> String {
    // Small FNV-1a hash → stable cache filename.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in title.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

async fn fetch_wiki_lang(client: &reqwest::Client, lang: &str, title: &str) -> Option<WikiInfo> {
    let enc = urlencoding::encode(title);
    let url = format!("https://{lang}.wikipedia.org/api/rest_v1/page/summary/{enc}?redirect=true");
    let resp = client
        .get(&url)
        .header("User-Agent", "DecompDeck/0.1 (decompilation launcher)")
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

/// Returns a cached-or-freshly-fetched Wikipedia summary for `title`, trying
/// Spanish first and falling back to English.
#[tauri::command]
pub async fn fetch_wiki(state: State<'_, AppState>, title: String) -> AppResult<Option<WikiInfo>> {
    let cache_dir = state.paths.data_dir.join("wiki_cache");
    std::fs::create_dir_all(&cache_dir)?;
    let cache_file = cache_dir.join(format!("{}.json", wiki_cache_key(&title)));
    if let Ok(bytes) = std::fs::read(&cache_file) {
        if let Ok(info) = serde_json::from_slice::<WikiInfo>(&bytes) {
            return Ok(Some(info));
        }
    }
    let info = match fetch_wiki_lang(&state.client, "es", &title).await {
        Some(i) => Some(i),
        None => fetch_wiki_lang(&state.client, "en", &title).await,
    };
    if let Some(ref i) = info {
        let _ = std::fs::write(&cache_file, serde_json::to_vec(i)?);
    }
    Ok(info)
}

/// Locates a console logo for `id` by reusing the user's existing ES-DE theme
/// art (their own files — nothing is bundled or downloaded). Returns an absolute
/// path the frontend loads through Tauri's asset protocol, preferring colored
/// "system logo" art over plain controller outlines.
#[tauri::command]
pub fn system_logo(id: String) -> Option<String> {
    let mut theme_roots: Vec<std::path::PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        theme_roots.push(home.join("ES-DE/themes"));
    }
    theme_roots.push(std::path::PathBuf::from("/usr/share/es-de/themes"));

    // Our catalog system ids don't always match ES-DE's logo filenames; map to
    // the ES-DE id(s) to try, in preference order.
    let candidates: Vec<&str> = match id.as_str() {
        "x360" => vec!["xbox360", "x360"],
        "pc" => vec!["windows", "pc"], // the Windows wordmark reads better than ES-DE's IBM "pc"
        other => vec![other],
    };

    // Sub-paths within a theme, most-preferred first (colored wordmark logos).
    let subs = [
        "system/logos/system-logo-color",
        "_inc/systems/logos",
        "system/logos",
        "system/controller-outline",
    ];

    // Prefer a colored logo for the aliased id across ALL themes before falling
    // back to a lesser sub-path.
    for cand in &candidates {
        for sub in subs {
            for root in &theme_roots {
                let Ok(themes) = std::fs::read_dir(root) else {
                    continue;
                };
                for theme in themes.flatten() {
                    let tdir = theme.path();
                    if !tdir.is_dir() {
                        continue;
                    }
                    for ext in ["svg", "png"] {
                        let p = tdir.join(sub).join(format!("{cand}.{ext}"));
                        if p.is_file() {
                            return Some(p.display().to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

#[derive(Serialize)]
pub struct PathsInfo {
    pub data_dir: String,
    pub portable: bool,
}

#[tauri::command]
pub fn get_paths_info(state: State<AppState>) -> PathsInfo {
    PathsInfo {
        data_dir: state.paths.data_dir.display().to_string(),
        portable: state.paths.is_portable(),
    }
}

#[tauri::command]
pub fn get_config(state: State<AppState>) -> AppResult<Config> {
    store::load_config(&state.paths)
}

#[tauri::command]
pub fn set_config(
    state: State<AppState>,
    github_token: Option<String>,
    catalog_url: Option<String>,
) -> AppResult<()> {
    let mut cfg = store::load_config(&state.paths)?;
    cfg.github_token = github_token.filter(|s| !s.is_empty());
    cfg.catalog_url = catalog_url.filter(|s| !s.is_empty());
    store::save_config(&state.paths, &cfg)
}

/// Toggles whether Windows-only builds are shown (to run via Wine/Proton).
#[tauri::command]
pub fn set_show_windows(state: State<AppState>, value: bool) -> AppResult<()> {
    let mut cfg = store::load_config(&state.paths)?;
    cfg.show_windows = value;
    store::save_config(&state.paths, &cfg)
}

/// Sets the Discord Application ID used for Rich Presence (None/empty disables).
#[tauri::command]
pub fn set_discord_app_id(state: State<AppState>, value: Option<String>) -> AppResult<()> {
    let mut cfg = store::load_config(&state.paths)?;
    cfg.discord_app_id = value.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    store::save_config(&state.paths, &cfg)
}

// ---------------------------------------------------------------------------
// Wine/Proton runners
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
pub struct Runner {
    /// Encoded id understood by `launch::launch_windows_with`: "wine",
    /// "umu:" (auto Proton), "umu:<path>" or "proton:<path>".
    pub id: String,
    pub label: String,
    pub kind: String, // "wine" | "proton"
}

fn have_cmd(cmd: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| dir.join(cmd).is_file())
        })
        .unwrap_or(false)
}

/// Detects the Wine/Proton runners available on this machine.
#[tauri::command]
pub fn list_runners() -> Vec<Runner> {
    // Order matters: the first entry is what "Automático" picks. These Windows
    // recomps use D3D12, which needs Proton (vkd3d-proton) — plain Wine shows a
    // black screen — so installed Proton builds come first, then umu-auto, then
    // Wine as a last resort.
    let mut runners = Vec::new();
    let wine = have_cmd("wine");
    let umu = have_cmd("umu-run");

    // Discover Proton installations (Steam + compatibilitytools.d), deduped by name.
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
            if !path.is_dir() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            // A Proton dir has a `proton` script.
            if !path.join("proton").is_file() {
                continue;
            }
            if !seen.insert(name.clone()) {
                continue;
            }
            // Prefer running through umu (handles prefix + env); else raw proton.
            let id = if umu {
                format!("umu:{}", path.display())
            } else {
                format!("proton:{}", path.display())
            };
            runners.push(Runner {
                id,
                label: format!("Proton: {name}"),
                kind: "proton".into(),
            });
        }
    }

    // umu auto-Proton (downloads GE-Proton on first run) after the installed ones.
    if umu {
        runners.push(Runner {
            id: "umu:".into(),
            label: "Proton (umu, automático)".into(),
            kind: "proton".into(),
        });
    }
    // Wine last: usually only works for simple apps, not D3D12 games.
    if wine {
        runners.push(Runner {
            id: "wine".into(),
            label: "Wine (sistema — sin D3D12)".into(),
            kind: "wine".into(),
        });
    }
    runners
}

/// Sets the global default runner (None = auto).
#[tauri::command]
pub fn set_runner(state: State<AppState>, runner: Option<String>) -> AppResult<()> {
    let mut cfg = store::load_config(&state.paths)?;
    cfg.wine_runner = runner.filter(|s| !s.is_empty());
    store::save_config(&state.paths, &cfg)
}

/// Sets a per-game runner override (None clears it → uses the default).
#[tauri::command]
pub fn set_game_runner(state: State<AppState>, id: String, runner: Option<String>) -> AppResult<()> {
    let mut cfg = store::load_config(&state.paths)?;
    match runner.filter(|s| !s.is_empty()) {
        Some(r) => {
            cfg.game_runners.insert(id, r);
        }
        None => {
            cfg.game_runners.remove(&id);
        }
    }
    store::save_config(&state.paths, &cfg)
}

/// Resolves the runner id to use for a game: per-game override, else the global
/// default, else the first available (wine, then umu).
fn resolve_runner(cfg: &Config, id: &str) -> Option<String> {
    if let Some(r) = cfg.game_runners.get(id) {
        return Some(r.clone());
    }
    if let Some(r) = &cfg.wine_runner {
        return Some(r.clone());
    }
    let runners = list_runners();
    runners.first().map(|r| r.id.clone())
}

// ---------------------------------------------------------------------------
// Catalog
// ---------------------------------------------------------------------------

/// Loads the catalog and returns only projects launchable on the current
/// platform, each enriched with local install status.
#[tauri::command]
pub fn list_catalog(state: State<AppState>) -> AppResult<CatalogView> {
    let triple = platform::current_triple();
    let catalog = store::load_catalog(&state.paths)?;
    let installed = store::load_installed(&state.paths)?;
    let show_windows = store::load_config(&state.paths)?.show_windows;
    // Windows fallback only makes sense when the host isn't already Windows.
    let win_fallback = show_windows && !triple.starts_with("windows");

    let mut views = Vec::new();
    for p in catalog.projects.into_iter() {
        let native_ok = supports_platform(&p, &triple);
        let win_ok = win_fallback
            && p.asset_rules.contains_key("windows-x86_64")
            && supports_platform(&p, "windows-x86_64");
        let entry = installed.get(&p.id);
        let installed = entry.is_some();

        // Show natively-supported games, Windows fallbacks (when enabled), and
        // anything already installed.
        if !native_ok && !win_ok && !installed {
            continue;
        }
        // A game is "windows" here when there's no native build but a Windows one.
        let is_windows = !native_ok && (win_ok || entry.map(|e| e.windows).unwrap_or(false));

        let installed_tag = entry.and_then(|e| e.installed_tag.clone());
        let update_available = match (&installed_tag, &p.cached) {
            (Some(cur), Some(c)) => c
                .latest_tag
                .as_ref()
                .map(|latest| latest != cur)
                .unwrap_or(false),
            _ => false,
        };
        views.push(ProjectView {
            installed,
            installed_tag,
            update_available,
            rom_configured: entry.and_then(|e| e.rom_path.as_ref()).is_some(),
            is_windows,
            project: p,
        });
    }

    Ok(CatalogView {
        platform: triple,
        systems: catalog.systems,
        projects: views,
    })
}

/// Default remote catalog: the `freeport-catalog` repo, refreshed daily by CI.
/// Users can override it with a custom URL in Ajustes.
pub const DEFAULT_CATALOG_URL: &str =
    "https://raw.githubusercontent.com/shadowlink/freeport-catalog/main/catalog.json";

/// Fetches a fresh catalog from the configured remote URL (or the default
/// `freeport-catalog` repo) and caches it.
#[tauri::command]
pub async fn refresh_catalog(state: State<'_, AppState>) -> AppResult<String> {
    let cfg = store::load_config(&state.paths)?;
    let url = cfg
        .catalog_url
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_CATALOG_URL.to_string());
    let text = state
        .client
        .get(&url)
        .header("User-Agent", "decompdeck")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let catalog: Catalog = serde_json::from_str(&text)?;
    store::save_catalog_cache(&state.paths, &catalog)?;
    Ok(catalog.updated_at)
}

// ---------------------------------------------------------------------------
// Install / uninstall
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize)]
struct InstallProgress {
    id: String,
    phase: String,
    downloaded: u64,
    total: u64,
}

fn find_project(catalog: &Catalog, id: &str) -> AppResult<Project> {
    catalog
        .projects
        .iter()
        .find(|p| p.id == id)
        .cloned()
        .ok_or_else(|| AppError::msg(format!("proyecto desconocido: {id}")))
}

#[tauri::command]
pub async fn install_project(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> AppResult<InstalledEntry> {
    let triple = platform::current_triple();
    let catalog = store::load_catalog(&state.paths)?;
    let project = find_project(&catalog, &id)?;
    let cfg = store::load_config(&state.paths)?;
    let token = cfg.github_token.as_deref();

    // Prefer the native asset; fall back to the Windows build (Wine/Proton) when
    // enabled and there's no native one.
    let native_ok = project.asset_rules.contains_key(&triple);
    let is_windows_install = !native_ok
        && !triple.starts_with("windows")
        && cfg.show_windows
        && project.asset_rules.contains_key("windows-x86_64");
    let install_triple = if native_ok {
        triple.clone()
    } else if is_windows_install {
        "windows-x86_64".to_string()
    } else {
        return Err(AppError::msg(format!(
            "{} no publica binario para {triple}",
            project.name
        )));
    };
    let rule = project.asset_rules.get(&install_triple).unwrap();

    // Resolve the release + platform asset live against GitHub.
    let releases = github::fetch_releases(&state.client, &project.repo.slug(), token).await?;
    let release = github::pick_release(
        &releases,
        &project.release_channel,
        project.rolling_tag.as_deref(),
    )
    .ok_or_else(|| AppError::msg("no se encontró una release adecuada"))?;
    let asset = github::pick_asset(&release, rule)?;

    // Prepare a clean install directory.
    let app_dir = state.paths.app_dir(&id);
    if app_dir.exists() {
        std::fs::remove_dir_all(&app_dir)?;
    }
    std::fs::create_dir_all(&app_dir)?;

    // Download.
    let archive = app_dir.join(&asset.name);
    let id_dl = id.clone();
    let app_dl = app.clone();
    install::download_to_file(&state.client, &asset.browser_download_url, &archive, |d, t| {
        let _ = app_dl.emit(
            "install://progress",
            InstallProgress {
                id: id_dl.clone(),
                phase: "download".into(),
                downloaded: d,
                total: t,
            },
        );
    })
    .await?;

    if install::is_archive(&asset.name) {
        // Extract, then discard the archive.
        let _ = app.emit(
            "install://progress",
            InstallProgress {
                id: id.clone(),
                phase: "extract".into(),
                downloaded: 0,
                total: 0,
            },
        );
        install::extract_archive(archive.clone(), app_dir.clone()).await?;
        let _ = std::fs::remove_file(&archive);
    } else {
        // Bare executable (AppImage, ELF, .x86_64…): install as-is and make it
        // runnable.
        install::make_executable(&archive);
    }

    // Record install.
    let mut installed = store::load_installed(&state.paths)?;
    let entry = InstalledEntry {
        installed_tag: Some(release.tag_name.clone()),
        published_at: release.published_at.clone(),
        install_path: app_dir.display().to_string(),
        rom_path: installed.get(&id).and_then(|e| e.rom_path.clone()),
        installed_at: Some(now_epoch()),
        windows: is_windows_install,
    };
    installed.insert(id.clone(), entry.clone());
    store::save_installed(&state.paths, &installed)?;

    let _ = app.emit(
        "install://progress",
        InstallProgress {
            id,
            phase: "done".into(),
            downloaded: 0,
            total: 0,
        },
    );
    Ok(entry)
}

#[tauri::command]
pub fn uninstall_project(state: State<AppState>, id: String) -> AppResult<()> {
    let app_dir = state.paths.app_dir(&id);
    if app_dir.exists() {
        std::fs::remove_dir_all(&app_dir)?;
    }
    let mut installed = store::load_installed(&state.paths)?;
    installed.remove(&id);
    store::save_installed(&state.paths, &installed)
}

// ---------------------------------------------------------------------------
// ROM + launch
// ---------------------------------------------------------------------------

/// Registers the user-provided ROM for a project. If the project expects the ROM
/// beside the binary under a specific name, it is copied there.
#[tauri::command]
pub fn set_rom(state: State<AppState>, id: String, rom_source: String) -> AppResult<()> {
    let catalog = store::load_catalog(&state.paths)?;
    let project = find_project(&catalog, &id)?;
    let mut installed = store::load_installed(&state.paths)?;
    let entry = installed
        .get_mut(&id)
        .ok_or_else(|| AppError::msg("el proyecto no está instalado"))?;

    let source = std::path::Path::new(&rom_source);
    if !source.exists() {
        return Err(AppError::msg("el archivo de ROM indicado no existe"));
    }

    // Copy the ROM next to the launch binary so the port finds it: either under
    // the exact filename it expects (e.g. Perfect Dark's pd.ntsc-final.z64) or,
    // failing that, under the ROM's original name (HarbourMasters ports scan
    // their own folder for a compatible ROM).
    let hint_os = if entry.windows { "windows" } else { std::env::consts::OS };
    let hint = project.launch.get(hint_os).and_then(|v| v.clone());
    let install_dir = std::path::Path::new(&entry.install_path);
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
        .or_else(|| {
            source
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
        })
        .ok_or_else(|| AppError::msg("nombre de ROM inválido"))?;
    let target = target_dir.join(target_name);
    std::fs::copy(source, &target)?;
    entry.rom_path = Some(target.display().to_string());
    store::save_installed(&state.paths, &installed)
}

#[derive(Clone, Serialize)]
struct GameExited {
    id: String,
}

#[tauri::command]
pub fn launch_project(app: AppHandle, state: State<AppState>, id: String) -> AppResult<u32> {
    let catalog = store::load_catalog(&state.paths)?;
    let project = find_project(&catalog, &id)?;
    let installed = store::load_installed(&state.paths)?;
    let entry = installed
        .get(&id)
        .ok_or_else(|| AppError::msg("el proyecto no está instalado"))?;
    let install_dir = std::path::Path::new(&entry.install_path);
    let hint_os = if entry.windows { "windows" } else { std::env::consts::OS };
    let hint = project.launch.get(hint_os).and_then(|v| v.clone());
    let bin = install::find_launch_binary(install_dir, hint.as_deref(), entry.windows)?;
    let mut child = if entry.windows {
        let cfg = store::load_config(&state.paths)?;
        let runner = resolve_runner(&cfg, &id).ok_or_else(|| {
            AppError::msg(
                "no hay ningún runner de Windows disponible. Instala Wine o umu-launcher (Proton).",
            )
        })?;
        let prefix = install_dir.join(".wineprefix");
        launch::launch_windows_with(&bin, &runner, &prefix)?
    } else {
        launch::launch_binary(&bin)?
    };
    let pid = child.id();

    // Discord Rich Presence: show "Jugando <juego>" while it runs.
    let cfg = store::load_config(&state.paths).unwrap_or_default();
    if let Some(app_id) = crate::discord::resolve_app_id(cfg.discord_app_id.as_deref()) {
        state
            .discord
            .set_playing(&app_id, &project.original_game, &project.name);
    }

    // Wait for the game to exit on a background thread, then tell the UI so the
    // TV/Big-Picture launcher can re-take fullscreen focus, and clear presence.
    let id2 = id.clone();
    std::thread::spawn(move || {
        let _ = child.wait();
        app.state::<AppState>().discord.clear();
        let _ = app.emit("game://exited", GameExited { id: id2 });
    });
    Ok(pid)
}

// ---------------------------------------------------------------------------
// Mods (Thunderstore)
// ---------------------------------------------------------------------------

/// The game's mods folder. For GameBanana/SoH-style ports it's a `mods` folder
/// next to the installed executable; for N64Recomp (Thunderstore) it's
/// `~/.config/<AppName>/mods` (AppName = the recomp's binary name).
fn mods_dir_for(
    project: &Project,
    source: &str,
    install_dir: Option<&std::path::Path>,
) -> AppResult<std::path::PathBuf> {
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

/// Install directory for a project, if installed.
fn install_dir_of(state: &AppState, id: &str) -> Option<std::path::PathBuf> {
    store::load_installed(&state.paths)
        .ok()?
        .get(id)
        .map(|e| std::path::PathBuf::from(&e.install_path))
}

/// Returns the mod list for a source (Thunderstore community or GameBanana game
/// id), using the in-memory cache when available.
async fn cached_mods(
    state: &AppState,
    source: &str,
    community: &str,
) -> AppResult<Vec<crate::mods::ModInfo>> {
    let key = format!("{source}:{community}");
    if let Ok(cache) = state.mods_cache.lock() {
        if let Some(list) = cache.get(&key) {
            return Ok(list.clone());
        }
    }
    let list = match source {
        "gamebanana" => crate::mods::fetch_gb_mods(&state.client, community).await?,
        _ => crate::mods::fetch_mods(&state.client, community).await?,
    };
    if let Ok(mut cache) = state.mods_cache.lock() {
        cache.insert(key, list.clone());
    }
    Ok(list)
}

#[tauri::command]
pub async fn list_mods(state: State<'_, AppState>, id: String) -> AppResult<Vec<crate::mods::ModInfo>> {
    let catalog = store::load_catalog(&state.paths)?;
    let project = find_project(&catalog, &id)?;
    let src = project
        .mods
        .as_ref()
        .ok_or_else(|| AppError::msg("este juego no tiene fuente de mods configurada"))?;
    cached_mods(&state, &src.source, &src.community).await
}

#[derive(Clone, Serialize)]
struct ModProgress {
    id: String,
    /// The mod the user clicked (so parallel installs map to the right card).
    target: String,
    pkg: String,
    index: usize,
    total: usize,
    downloaded: u64,
    total_bytes: u64,
    phase: String,
}

#[tauri::command]
pub async fn install_mod(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    full_name: String,
) -> AppResult<Vec<String>> {
    let catalog = store::load_catalog(&state.paths)?;
    let project = find_project(&catalog, &id)?;
    let src = project
        .mods
        .as_ref()
        .ok_or_else(|| AppError::msg("este juego no tiene fuente de mods configurada"))?
        .clone();
    let inst_dir = install_dir_of(&state, &id);
    let dir = mods_dir_for(&project, &src.source, inst_dir.as_deref())?;
    let all = cached_mods(&state, &src.source, &src.community).await?;
    let app2 = app.clone();
    let id2 = id.clone();
    let target = full_name.clone();
    let progress = move |pkg: &str, index, total, downloaded, total_bytes, phase: &str| {
        let _ = app2.emit(
            "mod://progress",
            ModProgress {
                id: id2.clone(),
                target: target.clone(),
                pkg: pkg.to_string(),
                index,
                total,
                downloaded,
                total_bytes,
                phase: phase.to_string(),
            },
        );
    };
    let files = if src.source == "gamebanana" {
        crate::mods::install_gb_mod(&state.client, &full_name, &dir, progress).await?
    } else {
        crate::mods::install_mod(&state.client, &all, &full_name, &dir, progress).await?
    };

    // Record the installed version + files so the UI can mark it installed,
    // detect updates, and remove it later.
    let version = all
        .iter()
        .find(|m| m.full_name == full_name)
        .map(|m| m.version.clone())
        .unwrap_or_default();
    let mut mstate = store::load_mod_state(&state.paths)?;
    mstate.entry(id.clone()).or_default().insert(
        full_name.clone(),
        store::InstalledMod {
            version,
            files: files.clone(),
        },
    );
    store::save_mod_state(&state.paths, &mstate)?;
    Ok(files)
}

/// Map of installed mod full_name -> installed version (for this game). The
/// frontend compares each against the latest version to flag updates.
#[tauri::command]
pub fn installed_mods(
    state: State<AppState>,
    id: String,
) -> AppResult<std::collections::HashMap<String, String>> {
    let mstate = store::load_mod_state(&state.paths)?;
    Ok(mstate
        .get(&id)
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.version.clone())).collect())
        .unwrap_or_default())
}

/// Removes a mod by full_name: deletes its files (except ones still used by
/// another installed mod) and forgets it.
#[tauri::command]
pub fn uninstall_mod(state: State<AppState>, id: String, full_name: String) -> AppResult<()> {
    let catalog = store::load_catalog(&state.paths)?;
    let project = find_project(&catalog, &id)?;
    let source = project.mods.as_ref().map(|m| m.source.clone()).unwrap_or_default();
    let inst_dir = install_dir_of(&state, &id);
    let dir = mods_dir_for(&project, &source, inst_dir.as_deref())?;

    let mut mstate = store::load_mod_state(&state.paths)?;
    let files = mstate
        .get(&id)
        .and_then(|m| m.get(&full_name))
        .map(|im| im.files.clone())
        .unwrap_or_default();

    // Files still referenced by OTHER installed mods must be kept.
    let keep: std::collections::BTreeSet<String> = mstate
        .get(&id)
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
        let name = std::path::Path::new(f).file_name().unwrap_or_default();
        let target = dir.join(name);
        if target.exists() {
            let _ = std::fs::remove_file(&target);
        }
    }
    if let Some(m) = mstate.get_mut(&id) {
        m.remove(&full_name);
    }
    store::save_mod_state(&state.paths, &mstate)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Update check
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct UpdateInfo {
    pub id: String,
    pub name: String,
    pub current: Option<String>,
    pub latest: String,
    pub update_available: bool,
}

/// Live-checks installed projects against GitHub. Errors on individual repos are
/// swallowed so one failure doesn't abort the whole check.
#[tauri::command]
pub async fn check_updates(state: State<'_, AppState>) -> AppResult<Vec<UpdateInfo>> {
    let catalog = store::load_catalog(&state.paths)?;
    let installed = store::load_installed(&state.paths)?;
    let cfg = store::load_config(&state.paths)?;
    let token = cfg.github_token.as_deref();

    let mut out = Vec::new();
    for (id, entry) in installed.iter() {
        let Ok(project) = find_project(&catalog, id) else {
            continue;
        };
        let releases = match github::fetch_releases(&state.client, &project.repo.slug(), token).await
        {
            Ok(r) => r,
            Err(_) => continue,
        };
        let Some(release) = github::pick_release(
            &releases,
            &project.release_channel,
            project.rolling_tag.as_deref(),
        ) else {
            continue;
        };
        // For rolling tags the tag never changes; compare published_at instead.
        let update = if project.release_channel == "rolling" {
            release.published_at != entry.published_at
        } else {
            Some(&release.tag_name) != entry.installed_tag.as_ref()
        };
        out.push(UpdateInfo {
            id: id.clone(),
            name: project.name.clone(),
            current: entry.installed_tag.clone(),
            latest: release.tag_name.clone(),
            update_available: update,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// TV mode + Sunshine (Moonlight)
// ---------------------------------------------------------------------------

/// True when the app was started with `--tv` (Sunshine launches it this way) so
/// the frontend opens straight into the Big-Picture/fullscreen UI.
#[tauri::command]
pub fn is_tv_mode() -> bool {
    std::env::args().any(|a| a == "--tv")
}

#[derive(Serialize)]
pub struct SunshineStatus {
    pub found: bool,
    pub added: bool,
    pub path: Option<String>,
}

fn sunshine_apps_path() -> Option<std::path::PathBuf> {
    let home = dirs::home_dir()?;
    let candidates = [
        home.join(".config/sunshine/apps.json"),
        home.join(".var/app/dev.lizardbyte.app.Sunshine/config/sunshine/apps.json"),
        home.join(".var/app/dev.lizardbyte.app.Sunshine/config/apps.json"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

/// The command Sunshine should run to stream Freeport in TV mode.
fn freeport_launch_cmd() -> String {
    if let Some(home) = dirs::home_dir() {
        let appimg = home.join("Applications/Freeport.AppImage");
        if appimg.exists() {
            return format!("{} --tv", appimg.display());
        }
    }
    match std::env::current_exe() {
        Ok(p) => format!("{} --tv", p.display()),
        Err(_) => "Freeport --tv".to_string(),
    }
}

#[tauri::command]
pub fn sunshine_status() -> SunshineStatus {
    let Some(path) = sunshine_apps_path() else {
        return SunshineStatus {
            found: false,
            added: false,
            path: None,
        };
    };
    let added = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("apps").and_then(|a| a.as_array()).cloned())
        .map(|apps| {
            apps.iter()
                .any(|a| a.get("name").and_then(|n| n.as_str()) == Some("Freeport"))
        })
        .unwrap_or(false);
    SunshineStatus {
        found: true,
        added,
        path: Some(path.display().to_string()),
    }
}

/// Registers Freeport as a Sunshine app (backs up apps.json first).
#[tauri::command]
pub fn add_to_sunshine() -> AppResult<String> {
    let path = sunshine_apps_path().ok_or_else(|| {
        AppError::msg("no se encontró la configuración de Sunshine (apps.json). ¿Está instalado?")
    })?;
    let content = std::fs::read_to_string(&path)?;
    let mut root: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| AppError::msg(format!("apps.json de Sunshine no válido: {e}")))?;

    let apps = root
        .get_mut("apps")
        .and_then(|a| a.as_array_mut())
        .ok_or_else(|| AppError::msg("apps.json no tiene la lista «apps»"))?;

    if apps
        .iter()
        .any(|a| a.get("name").and_then(|n| n.as_str()) == Some("Freeport"))
    {
        return Ok("Freeport ya estaba en Sunshine.".to_string());
    }

    let icon = dirs::home_dir()
        .map(|h| {
            h.join(".local/share/icons/hicolor/512x512/apps/freeport.png")
                .display()
                .to_string()
        })
        .unwrap_or_default();

    let mut entry = serde_json::Map::new();
    entry.insert("name".into(), serde_json::json!("Freeport"));
    entry.insert("cmd".into(), serde_json::json!(freeport_launch_cmd()));
    if !icon.is_empty() {
        entry.insert("image-path".into(), serde_json::json!(icon));
    }
    entry.insert("auto-detach".into(), serde_json::json!("false"));
    apps.push(serde_json::Value::Object(entry));

    // Backup, then write.
    let _ = std::fs::copy(&path, path.with_extension("json.bak"));
    std::fs::write(&path, serde_json::to_string_pretty(&root)?)?;
    Ok(format!(
        "Freeport añadido a Sunshine ({}). Reinicia Sunshine para que aparezca en Moonlight.",
        path.display()
    ))
}
