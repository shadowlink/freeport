// Freeport — native UI (Slint) on top of freeport-core (reused backend).
// Phase 1-2: catalog view + install/launch/rom/uninstall + on-demand thumbnails.

slint::include_modules!();


use freeport_core::model::{Catalog, Project};
use freeport_core::store::{self, Paths};
use freeport_core::mods::ModInfo;
use freeport_core::wiki::WikiInfo;
use freeport_core::{actions, gamepad, platform, ra, thumbs, update, wiki};
use slint::{Color, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel, Weak};
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::Path;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Recursively sums the size of a directory tree (best-effort).
fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for e in entries.flatten() {
            match e.file_type() {
                Ok(ft) if ft.is_dir() => total += dir_size(&e.path()),
                Ok(ft) if ft.is_file() => total += e.metadata().map(|m| m.len()).unwrap_or(0),
                _ => {}
            }
        }
    }
    total
}

fn fmt_bytes(b: u64) -> String {
    const U: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < 3 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{b} B") } else { format!("{v:.1} {}", U[i]) }
}

fn fmt_duration(secs: u64) -> String {
    let (h, m) = (secs / 3600, (secs % 3600) / 60);
    if h > 0 { format!("{h}h {m}m") } else if m > 0 { format!("{m}m") } else { format!("{secs}s") }
}

/// "2026-08-05T02:16:24Z" -> "5 ago 2026".
fn fmt_date(iso: &str) -> String {
    let date = iso.split('T').next().unwrap_or(iso);
    let parts: Vec<&str> = date.split('-').collect();
    if parts.len() != 3 {
        return date.to_string();
    }
    const MES: [&str; 12] = ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic"];
    let mi = parts[1].parse::<usize>().ok().filter(|x| (1..=12).contains(x)).map(|x| MES[x - 1]).unwrap_or(parts[1]);
    let di = parts[2].trim_start_matches('0');
    format!("{di} {mi} {}", parts[0])
}

fn fmt_ago(epoch: i64) -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let d = (now - epoch).max(0);
    if d < 60 { "hace un momento".into() }
    else if d < 3600 { format!("hace {}m", d / 60) }
    else if d < 86400 { format!("hace {}h", d / 3600) }
    else { format!("hace {}d", d / 86400) }
}

const LOGOS: &[(&str, &[u8])] = &[
    ("3ds", include_bytes!("../assets/logos/3ds.svg")),
    ("amiga", include_bytes!("../assets/logos/amiga.svg")),
    ("arcade", include_bytes!("../assets/logos/arcade.svg")),
    ("dc", include_bytes!("../assets/logos/dc.svg")),
    ("gb", include_bytes!("../assets/logos/gb.svg")),
    ("gba", include_bytes!("../assets/logos/gba.svg")),
    ("gc", include_bytes!("../assets/logos/gc.svg")),
    ("n64", include_bytes!("../assets/logos/n64.svg")),
    ("nds", include_bytes!("../assets/logos/nds.svg")),
    ("pc", include_bytes!("../assets/logos/pc.svg")),
    ("ps2", include_bytes!("../assets/logos/ps2.svg")),
    ("ps3", include_bytes!("../assets/logos/ps3.svg")),
    ("ps4", include_bytes!("../assets/logos/ps4.svg")),
    ("ps5", include_bytes!("../assets/logos/ps5.svg")),
    ("psp", include_bytes!("../assets/logos/psp.svg")),
    ("psx", include_bytes!("../assets/logos/psx.svg")),
    ("wii", include_bytes!("../assets/logos/wii.svg")),
    ("wiiu", include_bytes!("../assets/logos/wiiu.svg")),
    ("switch", include_bytes!("../assets/logos/switch.svg")),
    ("switch2", include_bytes!("../assets/logos/switch2.svg")),
    ("x360", include_bytes!("../assets/logos/x360.svg")),
    ("xbox", include_bytes!("../assets/logos/xbox.svg")),
];

/// One entry of the view history: which screen, which console, which game page.
#[derive(Clone, PartialEq, Debug)]
struct NavState {
    home_level: i32,
    settings: bool,
    library: bool,
    detail: Option<String>,
}

struct DetailState {
    id: String,
    wiki: Option<WikiInfo>,
    /// Transient side-panel feedback ("Añadido a Steam ✓").
    msg: String,
}

/// How long a catalog addition counts as "new" (badge + showcase strip).
const NEW_WINDOW_SECS: i64 = 7 * 86400;

/// Catalog ids → epoch when first seen (persisted across sessions).
fn seen_file(paths: &Paths) -> std::path::PathBuf {
    paths.data_dir.join("seen_catalog.json")
}

fn load_seen(paths: &Paths) -> std::collections::HashMap<String, i64> {
    let Ok(txt) = std::fs::read_to_string(seen_file(paths)) else {
        return Default::default();
    };
    if let Ok(m) = serde_json::from_str::<std::collections::HashMap<String, i64>>(&txt) {
        return m;
    }
    // Legacy format: plain id list → treat as long-known (epoch 0).
    serde_json::from_str::<Vec<String>>(&txt)
        .map(|v| v.into_iter().map(|id| (id, 0)).collect())
        .unwrap_or_default()
}

/// Stamps first-seen times: ids not in the file get "now" (they stay NEW for
/// [`NEW_WINDOW_SECS`]); a first-ever run records everything as old baseline.
fn save_seen(paths: &Paths, current: &[String]) {
    let mut m = load_seen(paths);
    let baseline = m.is_empty();
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    for id in current {
        m.entry(id.clone()).or_insert(if baseline { 0 } else { now });
    }
    if let Ok(json) = serde_json::to_string(&m) {
        let _ = std::fs::write(seen_file(paths), json);
    }
}

/// Whether a catalog entry still counts as a fresh addition.
fn is_new_id(seen: &std::collections::HashMap<String, i64>, id: &str, now: i64) -> bool {
    match seen.get(id) {
        None => !seen.is_empty(), // appeared mid-session (post-baseline)
        Some(0) => false,
        Some(t) => now - t < NEW_WINDOW_SECS,
    }
}

struct App {
    catalog: RefCell<Catalog>,
    triple: String,
    paths: Paths,
    client: reqwest::Client,
    logos: std::collections::HashMap<String, Image>,
    runners: Vec<actions::Runner>,
    busy: RefCell<HashSet<String>>,
    install_progress: RefCell<std::collections::HashMap<String, f32>>,
    /// Busy label override per project (e.g. "Importando parche 01.09").
    phase: RefCell<std::collections::HashMap<String, String>>,
    /// Last RomM search results shown in the chooser (by rom id).
    romm_candidates: RefCell<Vec<freeport_core::romm::Rom>>,
    /// Project the RomM chooser is for.
    romm_project: RefCell<String>,
    detail: RefCell<Option<DetailState>>,
    mods_cache: RefCell<std::collections::HashMap<String, Vec<ModInfo>>>,
    mod_busy: RefCell<HashSet<String>>,
    mod_progress: RefCell<std::collections::HashMap<String, (f32, String)>>,
    mod_icons: RefCell<std::collections::HashMap<String, Image>>,
    screens_cache: RefCell<std::collections::HashMap<String, Vec<String>>>,
    cover_cache: RefCell<std::collections::HashMap<String, Image>>,
    /// Full-resolution covers for the few boxes shown big (home carousel of
    /// one console, game page); the grid keeps the small thumbnails.
    cover_hd: RefCell<std::collections::HashMap<String, Image>>,
    /// «Actualizar todo»: (id, title, state, progress) of each game of the batch.
    bulk_items: RefCell<Vec<(String, String, i32, f32)>>,
    /// Covers being decoded right now ("th:<url>" / "hd:<url>").
    img_pending: RefCell<HashSet<String>>,
    /// Big covers the home carousel wants kept (around the selection).
    hd_wanted: RefCell<HashSet<String>>,
    /// Game list side panel: decoded art (logo / screenshot / fanart) by URL,
    /// Spanish Wikipedia text by project id, and the game it is showing.
    media: RefCell<std::collections::HashMap<String, Image>>,
    info_desc: RefCell<std::collections::HashMap<String, String>>,
    info_id: RefCell<String>,
    size_cache: RefCell<std::collections::HashMap<String, u64>>,
    // id -> (tag, published_at ISO, changelog body)
    changelog_cache: RefCell<std::collections::HashMap<String, (String, String, String)>>,
    pending_update: RefCell<Option<update::Update>>,
    /// Transient per-game launch state shown inline on the Play button:
    /// present+false = "Jugando…", present+true = launch failed.
    launching: RefCell<std::collections::HashMap<String, bool>>,
    /// Systems whose experimental ports are listed (mirror of the config,
    /// refreshed on every rebuild); `exp_all` = legacy global switch.
    exp_systems: RefCell<HashSet<String>>,
    exp_all: std::cell::Cell<bool>,
    /// What the open version chooser will do with the picked id.
    chooser_mode: RefCell<String>,
    /// Games whose last install attempt failed (shows "Reintentar" inline).
    install_error: RefCell<HashSet<String>>,
    /// Home carousel: the console and game in focus (by id, so a list that
    /// changes under a filter keeps the same selection), and the ids shown.
    home_sys_id: RefCell<String>,
    home_game_id: RefCell<String>,
    home_sys_ids: RefCell<Vec<String>>,
    home_game_ids: RefCell<Vec<String>>,
    /// View history for the mouse's back/forward buttons.
    nav_hist: RefCell<Vec<NavState>>,
    nav_pos: std::cell::Cell<usize>,
    /// Set while a history entry is being replayed (so it isn't pushed again).
    nav_applying: std::cell::Cell<bool>,
    /// Catalog ids → first-seen epoch (drives NUEVO badge + newcomers strip).
    seen: RefCell<std::collections::HashMap<String, i64>>,
    /// RetroAchievements progress per project id + badge art by URL.
    ra_cache: RefCell<std::collections::HashMap<String, ra::RaProgress>>,
    ra_badges: RefCell<std::collections::HashMap<String, Image>>,
    /// Per-game cancel flags for in-flight installs/updates.
    cancels: RefCell<std::collections::HashMap<String, std::sync::Arc<std::sync::atomic::AtomicBool>>>,
    /// Set while "update all" runs; cancelling stops the queue.
    bulk_cancel: RefCell<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>>,
}

/// Whether a newer release than the installed one exists, comparing by publish
/// date (robust: a tag mismatch alone would flag downgrades and stale catalogs).
fn has_update(entry: &freeport_core::model::InstalledEntry, cached: &Option<freeport_core::model::Cached>) -> bool {
    let Some(c) = cached else { return false };
    // A native install never "updates" to a Windows-only release: that would
    // swap a working binary for one that needs Wine/Proton (and, for ports
    // that compile the game on first run, redo that work under Wine). The
    // update shows up again when the project ships a native build.
    if !entry.windows && !c.platforms.is_empty() && !c.platforms.iter().any(|p| *p == platform::current_triple()) {
        return false;
    }
    // The other way round: a Windows build installed through Wine/Proton when
    // the project now ships a native one (TotK Recomp added Linux in 0.2.1)
    // is offered as an update, which reinstalls the native build.
    if entry.windows && !cfg!(windows) && c.platforms.iter().any(|p| *p == platform::current_triple()) {
        return true;
    }
    match (&entry.published_at, &c.published_at) {
        // Both dates present → newer catalog date means a real update.
        (Some(inst), Some(latest)) if !inst.is_empty() && !latest.is_empty() => latest.as_str() > inst.as_str(),
        // Fall back to tag inequality only when we can't compare dates.
        _ => match (entry.installed_tag.as_ref(), c.latest_tag.as_ref()) {
            (Some(cur), Some(l)) => l != cur,
            _ => false,
        },
    }
}

/// Whether Freeport's window is the active one. The controller is read
/// globally (gilrs), so without this a game launched from Freeport would also
/// steer Freeport — and start other games — while the player uses the pad.
static WINDOW_FOCUSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

thread_local! {
    static CLOCK_TIMER: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
    static REFRESH_SOON: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
    static SCROLLTEST_TIMER: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
    static RUNTIME: RefCell<Option<tokio::runtime::Handle>> = const { RefCell::new(None) };
    static UI: RefCell<Option<(Rc<App>, Weak<MainWindow>)>> = const { RefCell::new(None) };
    // Keeps the periodic update-check timer alive for the process lifetime.
    static UPDATE_TIMER: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
    // Same, for the periodic catalog refresh.
    static CATALOG_TIMER: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
}

fn parse_color(hex: &str) -> Color {
    let h = hex.trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("88"), 16).unwrap_or(0x88);
    if h.len() >= 6 {
        Color::from_rgb_u8(byte(0), byte(2), byte(4))
    } else {
        Color::from_rgb_u8(0x88, 0x88, 0x88)
    }
}

fn supports(p: &Project, triple: &str) -> bool {
    p.supports(triple)
}

/// Width/height of the original box of each system, as scanned by libretro
/// (medians measured over the catalog's covers). The grid, the TV shelves and
/// the game page size their frames with it, so an N64 box is wide, a PS1 jewel
/// case square and a PSP case tall, like on a real shelf. SteamGridDB systems
/// (Xbox 360, PS5, PC) use the 2:3 grid format.
fn box_aspect(system: &str) -> f32 {
    match system {
        "n64" => 1.43,
        "psx" | "gb" | "gba" | "dc" => 1.0,
        "nds" => 1.11,
        "3ds" => 1.14,
        "gc" | "wii" | "wiiu" | "ps2" | "ps3" | "xbox" | "arcade" | "ps4" => 0.71,
        "switch" | "switch2" => 0.61,
        "psp" => 0.58,
        _ => 0.667,
    }
}

/// Slot width of a box in the grid: every box in a row shares the grid's
/// height, so the width follows the system's box shape (an N64 box is wide, a
/// PSP case narrow), clamped so nothing gets absurdly thin or wide.

/// Every version of the same game as `id` that the user can see (installed
/// ones always count), in catalog order.
fn group_members(app: &App, id: &str, installed: &store::InstalledMap, show_windows: bool) -> Vec<Project> {
    let Some(p) = app.find(id) else { return Vec::new() };
    let key = p.game_key().to_string();
    app.catalog
        .borrow()
        .projects
        .iter()
        .filter(|r| r.game_key() == key && (r.id == p.id || app.visibility(r, installed, show_windows).0))
        .cloned()
        .collect()
}

fn version_rows(app: &App, members: &[Project], installed: &store::InstalledMap, show_windows: bool, active: &str) -> Vec<VersionRow> {
    members
        .iter()
        .map(|r| {
            let (_, rwin) = app.visibility(r, installed, show_windows);
            let mut plats: Vec<&str> = r
                .cached
                .as_ref()
                .map(|c| c.platforms.iter().map(|t| platform_label(t)).collect())
                .unwrap_or_default();
            plats.dedup();
            VersionRow {
                id: r.id.clone().into(),
                name: r.name.clone().into(),
                kind: if r.kind == "recompilation" { "RECOMP" } else { "PORT" }.into(),
                is_windows: rwin,
                installed: installed.contains_key(&r.id),
                active: r.id == active,
                platforms: plats.join(" · ").into(),
                meta: r.cached.as_ref().and_then(|c| c.latest_tag.clone()).unwrap_or_default().into(),
            }
        })
        .collect()
}

/// Opens the version chooser dialog.
fn open_chooser(app: &App, win: &MainWindow, mode: &str, title: &str, rows: Vec<VersionRow>) {
    *app.chooser_mode.borrow_mut() = mode.to_string();
    win.set_chooser_mode(mode.into());
    win.set_chooser_title(title.into());
    win.set_chooser_items(ModelRc::new(VecModel::from(rows)));
    win.set_chooser_remember(false);
    win.set_chooser_focus(0);
    win.set_chooser_visible(true);
}

/// Human label for an asset triple ("linux-x86_64" → "Linux").
fn platform_label(triple: &str) -> &'static str {
    if triple.starts_with("linux") {
        "Linux"
    } else if triple.starts_with("windows") {
        "Windows"
    } else if triple.starts_with("macos") {
        "macOS"
    } else {
        "Otro"
    }
}

impl App {
    fn visibility(&self, p: &Project, installed: &store::InstalledMap, show_windows: bool) -> (bool, bool) {
        let native = supports(p, &self.triple);
        let win_ok = show_windows
            && !self.triple.starts_with("windows")
            && p.asset_rules.contains_key("windows-x86_64")
            && supports(p, "windows-x86_64");
        let inst = installed.get(&p.id);
        // Experimental ports stay hidden unless the user opts in for their
        // system — installed ones too (off means off; the switch brings them back).
        if p.is_experimental() && !self.experimental_allowed(&p.system) {
            return (false, false);
        }
        (native || win_ok || inst.is_some(), !native && (win_ok || inst.map(|e| e.windows).unwrap_or(false)))
    }

    fn experimental_allowed(&self, system: &str) -> bool {
        self.exp_all.get() || self.exp_systems.borrow().contains(system)
    }

    fn find(&self, id: &str) -> Option<Project> {
        self.catalog.borrow().projects.iter().find(|p| p.id == id).cloned()
    }
}

fn load_logos(dir: &Path) -> std::collections::HashMap<String, Image> {
    let _ = std::fs::create_dir_all(dir);
    let mut map = std::collections::HashMap::new();
    for (id, bytes) in LOGOS {
        // Always rewrite: the bundled set changes between releases (e.g. the
        // switch to monochrome logos) and a stale cached file would win otherwise.
        let f = dir.join(format!("{id}.svg"));
        let _ = std::fs::write(&f, bytes);
        if let Ok(img) = Image::load_from_path(&f) {
            map.insert((*id).to_string(), img);
        }
    }
    map
}

fn rebuild(app: &App, win: &MainWindow) {
    let installed = store::load_installed(&app.paths).unwrap_or_default();
    let library = win.get_library_tab();
    let query = win.get_search_text().to_lowercase();
    let seen = app.seen.borrow();
    let now_secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let busy = app.busy.borrow();
    let catalog = app.catalog.borrow();
    let cfg = store::load_config(&app.paths).unwrap_or_default();
    let show_windows = cfg.show_windows;
    *app.exp_systems.borrow_mut() = cfg.experimental_systems.iter().cloned().collect();
    if let Ok(x) = std::env::var("FREEPORT_DEBUG_EXP") {
        app.exp_systems.borrow_mut().insert(x); // dev aid: show experimentals for a system without saving
    }
    app.exp_all.set(cfg.show_experimental && cfg.experimental_systems.is_empty());
    let favs: std::collections::HashSet<&str> = cfg.favorites.iter().map(|s| s.as_str()).collect();

    // Sidebar counts: games (grouped versions) in the catalog, installs in the library.
    // `settings_rows` lists every system with games (hidden experimentals included)
    // so the user can switch experimentals on per system.
    let mut settings_rows: Vec<SysRow> = Vec::new();
    for s in &catalog.systems {
        // Experimental games of this system that the switch would actually bring
        // on screen: switch off, (library: installed only), and not just another
        // version of a game that already has a visible stable card.
        let exp_hidden = if app.experimental_allowed(&s.id) {
            0
        } else {
            let visible_keys: HashSet<&str> = catalog
                .projects
                .iter()
                .filter(|p| p.system == s.id && !p.is_experimental() && app.visibility(p, &installed, show_windows).0)
                .filter(|p| !library || installed.contains_key(&p.id))
                .map(|p| p.game_key())
                .collect();
            let mut keys: HashSet<&str> = HashSet::new();
            for p in catalog
                .projects
                .iter()
                .filter(|p| p.system == s.id && p.is_experimental() && (!library || installed.contains_key(&p.id)))
            {
                if !visible_keys.contains(p.game_key()) {
                    keys.insert(p.game_key());
                }
            }
            keys.len()
        };
        let total_games: HashSet<&str> = catalog.projects.iter().filter(|p| p.system == s.id).map(|p| p.game_key()).collect();
        if !total_games.is_empty() {
            let logo = app.logos.get(&s.id).cloned();
            settings_rows.push(SysRow {
                id: s.id.clone().into(),
                name: s.name.clone().into(),
                count: total_games.len() as i32,
                color: parse_color(&s.color),
                logo: logo.clone().unwrap_or_default(),
                has_logo: logo.is_some(),
                exp_on: app.experimental_allowed(&s.id),
                exp_hidden: exp_hidden as i32,
            });
        }
    }

    // Every filter that decides whether a project shows up in the current view.
    // Returns Some(is_windows) when it passes.
    let passes = |p: &Project| -> Option<bool> {
        let (visible, is_win) = app.visibility(p, &installed, show_windows);
        if !visible
            || (library && !installed.contains_key(&p.id))
        {
            return None;
        }
        if !query.is_empty() {
            let sys_name = catalog.systems.iter().find(|s| s.id == p.system).map(|s| s.name.as_str()).unwrap_or("");
            let hay = format!(
                "{} {} {} {}",
                p.original_game, p.name, sys_name, p.genre.clone().unwrap_or_default()
            )
            .to_lowercase();
            if !hay.contains(&query) {
                return None;
            }
        }
        Some(is_win)
    };

    // One card per GAME everywhere (versions of the same game fold into one
    // card, see core::groups). In the library too: with several versions
    // installed the card asks which one to play/remove, or uses the default
    // chosen in the game page.
    let card_groups: Vec<freeport_core::groups::GameGroup> =
        freeport_core::groups::group_visible(&catalog.projects, &app.triple, |p| passes(p).is_some());

    // (favorite, name_lc, year, last_played_epoch, card) for sorting.
    let mut sortable: Vec<(bool, String, i64, i64, CardItem, String)> = Vec::new();
    for g in &card_groups {
        // The card speaks for the whole group but acts on ONE project: the
        // installed version played most recently, else the group's primary.
        let last_played = |id: &str| -> i64 {
            installed.get(id).and_then(|e| e.last_played.as_ref()).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0)
        };
        let p: &Project = g
            .members
            .iter()
            .copied()
            .filter(|m| installed.contains_key(&m.id))
            .max_by_key(|m| last_played(&m.id))
            .unwrap_or_else(|| g.primary());
        let is_win = passes(p).unwrap_or(false);
        let any_installed = g.members.iter().any(|m| installed.contains_key(&m.id));
        let any_update =
            g.members.iter().any(|m| installed.get(&m.id).map(|e| has_update(e, &m.cached)).unwrap_or(false));
        let any_busy = g.members.iter().any(|m| busy.contains(&m.id));
        let any_fav = g.members.iter().any(|m| favs.contains(m.id.as_str()));
        let any_new = g.members.iter().any(|m| is_new_id(&seen, &m.id, now_secs));
        let entry = installed.get(&p.id);
        let update = entry.map(|e| has_update(e, &p.cached)).unwrap_or(false);
        let sys_color = catalog
            .systems
            .iter()
            .find(|s| s.id == p.system)
            .map(|s| parse_color(&s.color))
            .unwrap_or(Color::from_rgb_u8(0x88, 0x88, 0x88));
        let title = if p.original_game.is_empty() { p.name.clone() } else { p.original_game.clone() };
        let year = p.year.unwrap_or(0) as i64;
        let last = last_played(&p.id);
        let name_lc = title.to_lowercase();
        let play_state = match app.launching.borrow().get(&p.id) {
            Some(false) => 1, // launching
            Some(true) => 2,  // failed
            None => 0,
        };
        let install_err = app.install_error.borrow().contains(&p.id);
        let latest_tag = p.cached.as_ref().and_then(|c| c.latest_tag.clone()).unwrap_or_default();
        // Installed → its tag; catalog card → the latest published tag.
        let version = entry
            .and_then(|e| e.installed_tag.clone())
            .unwrap_or_else(|| latest_tag.clone());
        sortable.push((
            any_fav,
            name_lc,
            year,
            last,
            CardItem {
                id: p.id.clone().into(),
                title: title.into(),
                // One quiet line under the art: the project, or how many there are.
                subtitle: if g.members.len() > 1 {
                    format!("{} versiones", g.members.len()).into()
                } else {
                    p.name.clone().into()
                },
                cover: app.cover_cached(p),
                installed: any_installed,
                is_windows: is_win,
                update_available: any_update,
                needs_rom: p.rom.mode == "copy",
                rom_ok: entry.and_then(|e| e.rom_path.as_ref()).is_some(),
                busy: any_busy,
                progress: app.install_progress.borrow().get(&p.id).copied().unwrap_or(0.0),
                kind: if p.kind == "recompilation" { "RECOMP" } else { "PORT" }.into(),
                sys_color,
                favorite: any_fav,
                play_state,
                install_error: install_err,
                version: version.into(),
                new_version: if update { latest_tag.into() } else { "".into() },
                meta: if last > 0 { format!("Jugado {}", fmt_ago(last)).into() } else { "".into() },
                is_new: any_new,
                versions: g.members.len() as i32,
                aspect: box_aspect(&p.system),
                experimental: p.is_experimental(),
                fit_cover: fit_for(&app.cover_cached(p), &p.system),
                size: app.size_label(&p.id),
                cx: 0.0,
            },
            p.system.clone(),
        ));
    }
    // Favorites first, then alphabetical — always.
    sortable.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let cards: Vec<(CardItem, String)> = sortable.into_iter().map(|t| (t.4, t.5)).collect();
    build_home(app, win, &catalog, &cards, &installed, query.is_empty());

    // Settings → disk usage per console (sizes come from `refresh_library_size`).
    {
        let sizes = app.size_cache.borrow();
        let mut disk_rows: Vec<DiskRow> = Vec::new();
        let mut disk_bytes: Vec<u64> = Vec::new();
        let mut total = 0u64;
        for s in &catalog.systems {
            let mut sum = 0u64;
            let mut n = 0;
            let mut games: Vec<(u64, DiskGame)> = Vec::new();
            for p in catalog.projects.iter().filter(|p| p.system == s.id && installed.contains_key(&p.id)) {
                n += 1;
                let b = sizes.get(&p.id).copied().unwrap_or(0);
                sum += b;
                let title = if p.original_game.is_empty() { p.name.clone() } else { p.original_game.clone() };
                // Two versions of one game: say which.
                let title = if catalog.projects.iter().filter(|q| q.game_key() == p.game_key() && installed.contains_key(&q.id)).count() > 1 {
                    format!("{title} ({})", p.name)
                } else {
                    title
                };
                games.push((b, DiskGame {
                    id: p.id.clone().into(),
                    title: title.into(),
                    version: installed.get(&p.id).and_then(|e| e.installed_tag.clone()).unwrap_or_default().into(),
                    size: if b > 0 { fmt_bytes(b).into() } else { "".into() },
                }));
            }
            games.sort_by_key(|g| std::cmp::Reverse(g.0));
            let games: Vec<DiskGame> = games.into_iter().map(|g| g.1).collect();
            if n == 0 {
                continue;
            }
            total += sum;
            let logo = app.logos.get(&s.id).cloned();
            disk_bytes.push(sum);
            disk_rows.push(DiskRow {
                id: s.id.clone().into(),
                name: s.name.clone().into(),
                logo: logo.clone().unwrap_or_default(),
                has_logo: logo.is_some(),
                count: n,
                size: if sum > 0 { fmt_bytes(sum).into() } else { "…".into() },
                frac: 0.0,
                games: ModelRc::new(VecModel::from(games)),
            });
        }
        // Biggest consoles first.
        let mut order: Vec<usize> = (0..disk_rows.len()).collect();
        order.sort_by(|a, b| disk_bytes[*b].cmp(&disk_bytes[*a]));
        let max = disk_bytes.iter().copied().max().unwrap_or(0).max(1);
        let disk_rows: Vec<DiskRow> = order
            .into_iter()
            .map(|i| DiskRow { frac: disk_bytes[i] as f32 / max as f32, ..disk_rows[i].clone() })
            .collect();
        win.set_disk_rows(ModelRc::new(VecModel::from(disk_rows)));
        win.set_disk_total(if total > 0 { fmt_bytes(total).into() } else { "".into() });
    }

    // Pending updates across ALL installed games (ignores tab/filter/search),
    // so the sidebar badge is always truthful.
    let pending = catalog
        .projects
        .iter()
        .filter(|p| installed.get(&p.id).map(|e| has_update(e, &p.cached)).unwrap_or(false))
        .count();

    win.set_exp_all_on(!settings_rows.is_empty() && settings_rows.iter().all(|r| r.exp_on));
    let half = settings_rows.len().div_ceil(2);
    let settings_b = settings_rows.split_off(half);
    win.set_settings_systems(ModelRc::new(VecModel::from(settings_rows)));
    win.set_settings_systems_b(ModelRc::new(VecModel::from(settings_b)));
    win.set_updates_pending(pending as i32);
}

impl App {
    /// Whether the cached art should be cropped (`cover`) rather than
    /// letterboxed: SteamGridDB grids are 2:3 while a Switch or PS4 box is
    /// not, so `contain` left black bands above and below.
    fn fit_cover(&self, p: &Project) -> bool {
        fit_for(&self.cover(p), &p.system)
    }

    /// "12.3 GB" for an installed game whose size is known, else "".
    fn size_label(&self, id: &str) -> SharedString {
        match self.size_cache.borrow().get(id) {
            Some(b) if *b > 0 => fmt_bytes(*b).into(),
            _ => "".into(),
        }
    }

    /// The cover at its original resolution if it's already decoded; else the
    /// thumbnail (or nothing) now, and the big one is decoded (downloaded
    /// first if needed) in the background and patched into the screen.
    fn cover_hd(&self, p: &Project) -> Image {
        let Some(url) = cover_url(p) else { return Image::default() };
        if let Some(img) = self.cover_hd.borrow().get(&url) {
            return img.clone();
        }
        request_cover(self, &url, true);
        self.cover(p)
    }

    /// The thumbnail if it's already decoded; else nothing now and a
    /// background decode. Never reads or decodes a file on the UI thread
    /// (with ~500 covers that froze the first visit to Explorar).
    fn cover(&self, p: &Project) -> Image {
        let Some(url) = cover_url(p) else { return Image::default() };
        if let Some(img) = self.cover_cache.borrow().get(&url) {
            return img.clone();
        }
        request_cover(self, &url, false);
        Image::default()
    }

    /// The thumbnail only if it's already in memory (no request).
    fn cover_cached(&self, p: &Project) -> Image {
        cover_url(p).and_then(|u| self.cover_cache.borrow().get(&u).cloned()).unwrap_or_default()
    }
}

fn cover_url(p: &Project) -> Option<String> {
    p.box_art.as_ref().or(p.cover_url.as_ref()).cloned()
}

/// Crop instead of letterbox when the art's shape isn't the box's.
fn fit_for(img: &Image, system: &str) -> bool {
    let sz = img.size();
    if sz.width == 0 || sz.height == 0 {
        return false;
    }
    let ia = sz.width as f32 / sz.height as f32;
    (ia - box_aspect(system)).abs() > 0.05
}

/// Decodes a cover (thumbnail or big) on a worker thread and, back on the UI
/// thread, caches it and patches the boxes on screen. A big cover that isn't
/// on disk yet is downloaded first.
fn request_cover(app: &App, url: &str, hd: bool) {
    let key = format!("{}{url}", if hd { "hd:" } else { "th:" });
    if !app.img_pending.borrow_mut().insert(key.clone()) {
        return;
    }
    let Some(h) = RUNTIME.with(|r| r.borrow().clone()) else {
        app.img_pending.borrow_mut().remove(&key);
        return;
    };
    let path = if hd { thumbs::hd_path_for(&app.paths, url) } else { thumbs::path_for(&app.paths, url) };
    let (client, paths, url) = (app.client.clone(), app.paths.clone(), url.to_string());
    h.spawn(async move {
        if !path.exists() {
            // Thumbnails come from spawn_missing_thumbs; big covers on demand.
            if !hd || thumbs::get_hd(&client, &paths, &url).await.is_err() {
                let _ = slint::invoke_from_event_loop(move || {
                    UI.with(|u| {
                        if let Some((app, _)) = &*u.borrow() {
                            app.img_pending.borrow_mut().remove(&key);
                        }
                    });
                });
                return;
            }
        }
        let px = tokio::task::spawn_blocking(move || thumbs::load_rgba(&path, if hd { 1200 } else { 640 }))
            .await
            .ok()
            .flatten();
        let _ = slint::invoke_from_event_loop(move || {
            UI.with(|u| {
                let Some((app, weak)) = &*u.borrow() else { return };
                app.img_pending.borrow_mut().remove(&key);
                let Some((w, hgt, buf)) = px else { return };
                let img = Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&buf, w, hgt));
                if hd {
                    let mut cache = app.cover_hd.borrow_mut();
                    // Bounded: only the big boxes around the selection stay.
                    if cache.len() >= 40 {
                        let wanted = app.hd_wanted.borrow();
                        cache.retain(|k, _| wanted.contains(k));
                    }
                    cache.insert(url.clone(), img);
                } else {
                    app.cover_cache.borrow_mut().insert(url.clone(), img);
                }
                if let Some(w) = weak.upgrade() {
                    patch_covers(app, &w, &url);
                }
            });
        });
    });
}

/// Puts a freshly decoded cover into the boxes that show it, in place (no
/// rebuild): the home carousel, its preview strip, the console cards and the
/// game page.
fn patch_covers(app: &App, w: &MainWindow, url: &str) {
    let sel = w.get_home_game().max(0) as usize;
    let patch = |model: ModelRc<CardItem>, near_hd: bool| {
        for i in 0..model.row_count() {
            let Some(mut row) = model.row_data(i) else { continue };
            let Some(p) = app.find(row.id.as_str()) else { continue };
            if cover_url(&p).as_deref() != Some(url) {
                continue;
            }
            let want_hd = near_hd && i.abs_diff(sel) <= HD_RADIUS;
            let img = if want_hd {
                app.cover_hd.borrow().get(url).cloned().unwrap_or_else(|| app.cover_cached(&p))
            } else {
                app.cover_cached(&p)
            };
            if img != row.cover {
                row.fit_cover = fit_for(&img, &p.system);
                row.cover = img;
                model.set_row_data(i, row);
            }
        }
    };
    patch(w.get_home_games(), true);
    patch(w.get_home_preview(), false);
    let systems = w.get_home_systems();
    for i in 0..systems.row_count() {
        let Some(row) = systems.row_data(i) else { continue };
        let covers = row.covers.clone();
        for j in 0..covers.row_count() {
            let Some(mut c) = covers.row_data(j) else { continue };
            if let Some(p) = app.find(c.id.as_str()) {
                if cover_url(&p).as_deref() == Some(url) {
                    c.cover = app.cover_cached(&p);
                    c.fit_cover = fit_for(&c.cover, &p.system);
                    covers.set_row_data(j, c);
                }
            }
        }
    }
    if w.get_detail_visible() {
        schedule_refresh();
    }
}

/// Big covers are kept for the boxes this close to the selection.
const HD_RADIUS: usize = 3;

/// After the selection moves inside a console: big covers for the boxes
/// around it (decoded in the background), thumbnails further out.
fn home_window(app: &App, w: &MainWindow) {
    let games = w.get_home_games();
    let sel = w.get_home_game().max(0) as usize;
    let mut wanted = HashSet::new();
    for i in 0..games.row_count() {
        let Some(mut row) = games.row_data(i) else { continue };
        let Some(p) = app.find(row.id.as_str()) else { continue };
        let Some(url) = cover_url(&p) else { continue };
        let img = if i.abs_diff(sel) <= HD_RADIUS && w.get_home_level() == 1 {
            wanted.insert(url.clone());
            app.cover_hd(&p)
        } else {
            app.cover(&p)
        };
        if img != row.cover {
            row.fit_cover = fit_for(&img, &p.system);
            row.cover = img;
            games.set_row_data(i, row);
        }
    }
    *app.hd_wanted.borrow_mut() = wanted;
    home_info(app, w);
}

/// Side panel of the game list (ES-DE style): logo, screenshot, fanart,
/// facts and description of the selected game. Art and text not loaded yet
/// are fetched in the background, a moment after the selection settles.
fn home_info(app: &App, w: &MainWindow) {
    let sel = w.get_home_game().max(0) as usize;
    let id = app.home_game_ids.borrow().get(sel).cloned().unwrap_or_default();
    let Some(p) = app.find(&id) else {
        w.set_home_info(HomeInfo::default());
        return;
    };
    let changed = *app.info_id.borrow() != id;
    *app.info_id.borrow_mut() = id.clone();
    let lb = p.lb.clone().unwrap_or_default();
    let media = app.media.borrow();
    let pick = |u: &Option<String>| u.as_ref().and_then(|u| media.get(u).cloned());
    let logo = pick(&lb.logo);
    let shot = pick(&lb.snap).or_else(|| pick(&lb.title));
    let art = pick(&lb.fanart).or_else(|| pick(&lb.snap));
    let mut facts: Vec<SharedString> = Vec::new();
    if let Some(y) = p.year {
        facts.push(y.to_string().into());
    }
    if let Some(d) = p.developer.as_ref().filter(|d| !d.is_empty()) {
        facts.push(d.clone().into());
    }
    if let Some(g) = p.genre.as_ref().filter(|g| !g.is_empty()) {
        facts.push(g.clone().into());
    }
    match lb.players {
        Some(1) => facts.push("1 jugador".into()),
        Some(n) if n > 1 => facts.push(format!("1–{n} jugadores").into()),
        _ => {}
    }
    if lb.coop {
        facts.push("Cooperativo".into());
    }
    let desc = app
        .info_desc
        .borrow()
        .get(&id)
        .cloned()
        .unwrap_or_default();
    w.set_home_info(HomeInfo {
        has_logo: logo.is_some(),
        logo: logo.unwrap_or_default(),
        has_shot: shot.is_some(),
        shot: shot.unwrap_or_default(),
        has_art: art.is_some(),
        art: art.unwrap_or_default(),
        facts: ModelRc::new(VecModel::from(facts)),
        rating: lb.rating.unwrap_or(0.0),
        desc: desc.into(),
    });
    drop(media);
    let _ = changed;
    // Fetch what is missing, once the selection has rested for a moment
    // (scrolling fast through a list doesn't download every game's art).
    let urls: Vec<String> = [&lb.logo, &lb.snap, &lb.title, &lb.fanart]
        .into_iter()
        .flatten()
        .filter(|u| !app.media.borrow().contains_key(*u))
        .cloned()
        .collect();
    let need_desc = !app.info_desc.borrow().contains_key(&id);
    if urls.is_empty() && !need_desc {
        return;
    }
    // One request per game at a time.
    let key = format!("info:{id}");
    if !app.img_pending.borrow_mut().insert(key.clone()) {
        return;
    }
    let Some(h) = RUNTIME.with(|r| r.borrow().clone()) else {
        app.img_pending.borrow_mut().remove(&key);
        return;
    };
    let (client, paths) = (app.client.clone(), app.paths.clone());
    let wiki_title = p.wiki.clone();
    let overview = lb.overview.clone().unwrap_or_default();
    h.spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(140)).await;
        let still = {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let want = id.clone();
            let _ = slint::invoke_from_event_loop(move || {
                let ok = UI.with(|u| u.borrow().as_ref().map(|(a, _)| *a.info_id.borrow() == want).unwrap_or(false));
                let _ = tx.send(ok);
            });
            rx.await.unwrap_or(false)
        };
        if !still {
            let _ = slint::invoke_from_event_loop(move || {
                UI.with(|u| {
                    if let Some((app, _)) = &*u.borrow() {
                        app.img_pending.borrow_mut().remove(&key);
                    }
                });
            });
            return;
        }
        let mut imgs: Vec<(String, u32, u32, Vec<u8>)> = Vec::new();
        for u in urls {
            if let Ok(path) = thumbs::get_media(&client, &paths, &u).await {
                if let Ok(Some((wd, ht, buf))) = tokio::task::spawn_blocking(move || thumbs::load_rgba(&path, 1600)).await {
                    imgs.push((u, wd, ht, buf));
                }
            }
        }
        let mut desc = None;
        if need_desc {
            let wiki = match wiki_title {
                Some(t) => wiki::fetch(&client, &paths, &t).await,
                None => None,
            };
            desc = Some(wiki.map(|w| w.extract).filter(|e| !e.trim().is_empty()).unwrap_or(overview));
        }
        let _ = slint::invoke_from_event_loop(move || {
            UI.with(|u| {
                let Some((app, weak)) = &*u.borrow() else { return };
                app.img_pending.borrow_mut().remove(&key);
                {
                    let mut m = app.media.borrow_mut();
                    if m.len() > 24 {
                        m.clear();
                    }
                    for (url, wd, ht, buf) in imgs {
                        m.insert(url, Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&buf, wd, ht)));
                    }
                }
                if let Some(d) = desc {
                    app.info_desc.borrow_mut().insert(id.clone(), d);
                }
                if *app.info_id.borrow() == id {
                    if let Some(w) = weak.upgrade() {
                        home_info(app, &w);
                    }
                }
            });
        });
    });
}

fn build_detail(app: &App, win: &MainWindow) {
    let state = app.detail.borrow();
    let Some(state) = state.as_ref() else {
        win.set_detail_visible(false);
        return;
    };
    let Some(p) = app.find(&state.id) else {
        win.set_detail_visible(false);
        return;
    };
    let installed = store::load_installed(&app.paths).unwrap_or_default();
    let show_windows = store::load_config(&app.paths).map(|c| c.show_windows).unwrap_or(false);
    let catalog = app.catalog.borrow();
    let entry = installed.get(&p.id);
    let (_, is_win) = app.visibility(&p, &installed, show_windows);
    let update = entry.map(|e| has_update(e, &p.cached)).unwrap_or(false);
    let sys = catalog.systems.iter().find(|s| s.id == p.system);
    let sys_color = sys.map(|s| parse_color(&s.color)).unwrap_or(Color::from_rgb_u8(0x88, 0x88, 0x88));

    // "Ficha técnica" side panel: structured label/value rows.
    let white = Color::from_rgb_u8(0xff, 0xff, 0xff);
    let latest_tag = p.cached.as_ref().and_then(|c| c.latest_tag.clone()).unwrap_or_default();
    let installed_tag = entry.and_then(|e| e.installed_tag.clone()).unwrap_or_default();
    let mut facts: Vec<FactRow> = Vec::new();
    let mut fact = |label: &str, value: String, accent: Color| {
        if !value.is_empty() {
            facts.push(FactRow { label: label.into(), value: value.into(), accent });
        }
    };
    if let Some(s) = sys {
        fact("Sistema", s.name.clone(), sys_color);
    }
    fact(
        "Cómo está hecho",
        if p.kind == "recompilation" { "Traducido del juego original" } else { "Reconstruido para PC" }.into(),
        if p.kind == "recompilation" { parse_color("#2fd3ef") } else { parse_color("#3fa7e0") },
    );
    if let Some(y) = p.year {
        fact("Año", format!("{y}"), white);
    }
    fact("Desarrollador", p.developer.clone().unwrap_or_default(), white);
    fact("Género", p.genre.clone().unwrap_or_default(), white);
    fact(
        "Estado",
        match p.tier() {
            "curado" => "Muy probado",
            "experimental" => "Versión de pruebas",
            _ => "Funciona bien",
        }
        .into(),
        match p.tier() {
            "curado" => parse_color("#4de1c1"),
            "experimental" => parse_color("#ff9f43"),
            _ => white,
        },
    );
    if !installed_tag.is_empty() {
        fact("Versión instalada", installed_tag.clone(), parse_color("#4de1c1"));
    }
    if !latest_tag.is_empty() {
        fact(
            "Última versión",
            latest_tag.clone(),
            if update { parse_color("#ffd166") } else { white },
        );
    }
    if is_win {
        fact("Funciona con", "Compatibilidad de Windows (Proton)".into(), parse_color("#3fa7e0"));
    }

    let related: Vec<CardItem> = catalog
        .projects
        .iter()
        .filter(|r| {
            r.id != p.id
                && r.system == p.system
                && r.game_key() != p.game_key() // other versions live in the selector
                && app.visibility(r, &installed, show_windows).0
        })
        .take(12)
        .map(|r| CardItem {
            id: r.id.clone().into(),
            title: (if r.original_game.is_empty() { r.name.clone() } else { r.original_game.clone() }).into(),
            subtitle: r.name.clone().into(),
            cover: app.cover(r),
            installed: installed.contains_key(&r.id),
            is_windows: false,
            update_available: false,
            needs_rom: false,
            rom_ok: false,
            busy: false,
            progress: 0.0,
            kind: "".into(),
            sys_color,
            favorite: false,
            play_state: 0,
            install_error: false,
            version: "".into(),
            new_version: "".into(),
            meta: "".into(),
            is_new: false,
                    versions: 1,
                    aspect: box_aspect(&r.system),
        experimental: r.is_experimental(),
        fit_cover: app.fit_cover(r),
        size: app.size_label(&r.id),
        cx: 0.0,
        })
        .collect();

    // Versions of the same game (this one included), for the hero selector.
    let versions: Vec<VersionRow> = catalog
        .projects
        .iter()
        .filter(|r| r.game_key() == p.game_key() && (r.id == p.id || app.visibility(r, &installed, show_windows).0))
        .map(|r| {
            let (_, rwin) = app.visibility(r, &installed, show_windows);
            let mut plats: Vec<&str> = r
                .cached
                .as_ref()
                .map(|c| c.platforms.iter().map(|t| platform_label(t)).collect())
                .unwrap_or_default();
            plats.dedup();
            VersionRow {
                id: r.id.clone().into(),
                name: r.name.clone().into(),
                kind: if r.kind == "recompilation" { "RECOMP" } else { "PORT" }.into(),
                is_windows: rwin,
                installed: installed.contains_key(&r.id),
                active: r.id == p.id,
                platforms: plats.join(" · ").into(),
                meta: r.cached.as_ref().and_then(|c| c.latest_tag.clone()).unwrap_or_default().into(),
            }
        })
        .collect();

    // "Al jugar desde la tarjeta": ask, or one of the versions (same order as `versions`).
    let pref = store::load_config(&app.paths).ok().and_then(|c| c.preferred_version.get(p.game_key()).cloned());
    let mut default_labels: Vec<SharedString> = vec!["Preguntar".into()];
    default_labels.extend(versions.iter().map(|v| v.name.clone()));
    let default_index = pref
        .and_then(|id| versions.iter().position(|v| v.id.as_str() == id))
        .map(|i| i as i32 + 1)
        .unwrap_or(0);

    let mut mod_rows: Vec<ModRow> = Vec::new();
    if p.mods.is_some() {
        let inst = actions::installed_mods(&app.paths, &p.id);
        let mbusy = app.mod_busy.borrow();
        let mprog = app.mod_progress.borrow();
        let icons = app.mod_icons.borrow();
        let make_row = |m: &ModInfo, installed: bool, update: bool| {
            let icon = m.icon_url.as_ref().and_then(|u| icons.get(u).cloned());
            let prog = mprog.get(&m.full_name);
            ModRow {
                full_name: m.full_name.clone().into(),
                name: m.name.replace('_', " ").into(),
                owner: m.owner.clone().into(),
                downloads: format!("{}", m.downloads).into(),
                description: m.description.clone().into(),
                icon: icon.clone().unwrap_or_default(),
                has_icon: icon.is_some(),
                installed,
                update,
                busy: mbusy.contains(&m.full_name),
                progress: prog.map(|(p, _)| *p).unwrap_or(0.0),
                phase: prog.map(|(_, ph)| ph.clone()).unwrap_or_default().into(),
            }
        };
        let list = app.mods_cache.borrow();
        let list = list.get(&p.id).cloned().unwrap_or_default();
        let in_list: HashSet<String> = list.iter().map(|m| m.full_name.clone()).collect();
        // Installed mods first (so previously-installed ones are always visible,
        // even if they're not in the fetched catalog page).
        for (full_name, ver) in &inst {
            let catalog_mod = list.iter().find(|m| &m.full_name == full_name);
            let update = catalog_mod.map(|m| &m.version != ver).unwrap_or(false);
            match catalog_mod {
                Some(m) => mod_rows.push(make_row(m, true, update)),
                None => {
                    // Installed but not in the fetched list — synthesize a minimal row.
                    let name = full_name.rsplit('-').next().unwrap_or(full_name).replace('_', " ");
                    mod_rows.push(ModRow {
                        full_name: full_name.clone().into(),
                        name: name.into(),
                        owner: full_name.split('-').next().unwrap_or("").into(),
                        downloads: "".into(),
                        description: "Instalado".into(),
                        icon: Default::default(),
                        has_icon: false,
                        installed: true,
                        update: false,
                        busy: mbusy.contains(full_name),
                        progress: mprog.get(full_name).map(|(p, _)| *p).unwrap_or(0.0),
                        phase: mprog.get(full_name).map(|(_, ph)| ph.clone()).unwrap_or_default().into(),
                    });
                }
            }
        }
        // Then the rest of the catalog (not-installed), most popular first.
        for m in list.iter().take(80) {
            if in_list.contains(&m.full_name) && inst.contains_key(&m.full_name) {
                continue; // already shown above
            }
            mod_rows.push(make_row(m, false, false));
        }
    }

    // RetroAchievements: summary line + badge grid (12 per row).
    let mut ra_summary = String::new();
    let mut badge_rows: Vec<ModelRc<RaBadge>> = Vec::new();
    if let Some(prog) = app.ra_cache.borrow().get(&p.id) {
        ra_summary = format!(
            "{}/{} logros · {}/{} puntos",
            prog.earned, prog.total, prog.points_earned, prog.points_total
        );
        let icons = app.ra_badges.borrow();
        let badges: Vec<RaBadge> = prog
            .achievements
            .iter()
            .map(|a| {
                let img = icons.get(&ra::badge_url(&a.badge_name, a.unlocked)).cloned();
                RaBadge { img: img.clone().unwrap_or_default(), has_img: img.is_some(), unlocked: a.unlocked }
            })
            .collect();
        badge_rows = badges
            .chunks(12)
            .map(|c| ModelRc::new(VecModel::from(c.to_vec())))
            .collect();
    }

    let mut screens: Vec<ScreenShot> = Vec::new();
    if let Some(list) = app.screens_cache.borrow().get(&p.id) {
        for path in list {
            let img = Image::load_from_path(Path::new(path)).unwrap_or_default();
            screens.push(ScreenShot { img, path: path.clone().into() });
        }
    }

    let title = if p.original_game.is_empty() { p.name.clone() } else { p.original_game.clone() };
    // Play stats (only for installed games): play time · last played · size.
    let stats = match entry {
        Some(e) => {
            let mut parts: Vec<String> = Vec::new();
            if e.play_secs > 0 {
                parts.push(format!("Jugado {}", fmt_duration(e.play_secs)));
            }
            if let Some(lp) = e.last_played.as_ref().and_then(|s| s.parse::<i64>().ok()) {
                parts.push(format!("Última vez {}", fmt_ago(lp)));
            }
            let size = app.size_cache.borrow().get(&p.id).copied().unwrap_or(0);
            let size = if size == 0 {
                let s = dir_size(Path::new(&e.install_path));
                app.size_cache.borrow_mut().insert(p.id.clone(), s);
                s
            } else {
                size
            };
            if size > 0 {
                parts.push(fmt_bytes(size));
            }
            parts.join("  ·  ")
        }
        None => String::new(),
    };
    // Plain-language status line and the "add your game" action.
    let busy_now = app.busy.borrow().contains(&p.id);
    let rom_ok = entry.and_then(|e| e.rom_path.as_ref()).is_some();
    let add_kind = if entry.is_none() {
        ""
    } else if p.rom.mode == "copy" {
        "rom"
    } else if p.system == "ps4" {
        "pkg"
    } else if p.system == "switch" && p.rom.required {
        "switch"
    } else {
        ""
    };
    let add_label = match (add_kind, rom_ok) {
        ("", _) => "",
        ("rom", false) => "Elegir tu ROM",
        ("rom", true) => "Cambiar la ROM",
        (_, false) => "Añadir tu copia del juego",
        (_, true) => "Volver a añadir el juego",
    };
    let (status, status_color) = if busy_now {
        (String::new(), white)
    } else if entry.is_none() {
        (
            if p.is_experimental() { "Disponible para descargar · versión de pruebas".to_string() } else { "Disponible para descargar".to_string() },
            white,
        )
    } else if !add_kind.is_empty() && !rom_ok {
        ("Falta añadir tu copia del juego".to_string(), parse_color("#ffd166"))
    } else if update {
        ("Hay una versión nueva".to_string(), parse_color("#ffd166"))
    } else if app.install_error.borrow().contains(&p.id) {
        ("La última descarga falló".to_string(), parse_color("#ff6b7d"))
    } else {
        ("Listo para jugar".to_string(), parse_color("#4de1c1"))
    };
    // Sections of the page, only the ones with something to show.
    let mut tabs: Vec<TabItem> = vec![TabItem { key: "info".into(), label: "Información".into() }];
    if !p.rom.notes.is_empty() {
        tabs.push(TabItem { key: "setup".into(), label: "Cómo se instala".into() });
    }
    if app.changelog_cache.borrow().get(&p.id).map(|(_, _, b)| !b.is_empty()).unwrap_or(false) {
        tabs.push(TabItem { key: "news".into(), label: "Novedades".into() });
    }
    if !screens.is_empty() {
        tabs.push(TabItem { key: "screens".into(), label: "Capturas".into() });
    }
    if !ra_summary.is_empty() {
        tabs.push(TabItem { key: "ra".into(), label: "Logros".into() });
    }
    if !mod_rows.is_empty() {
        tabs.push(TabItem { key: "mods".into(), label: "Mods".into() });
    }
    tabs.push(TabItem { key: "more".into(), label: "Opciones".into() });
    let data = DetailData {
        id: p.id.clone().into(),
        title: title.into(),
        subtitle: p.name.clone().into(),
        cover: app.cover_hd(&p),
        installed: entry.is_some(),
        is_windows: is_win,
        update_available: update,
        needs_rom: p.rom.mode == "copy",
        rom_ok: entry.and_then(|e| e.rom_path.as_ref()).is_some(),
        is_ps4: p.system == "ps4",
        is_switch: p.system == "switch",
        phase: app.phase.borrow().get(&p.id).cloned().unwrap_or_default().into(),
        busy: app.busy.borrow().contains(&p.id),
        progress: app.install_progress.borrow().get(&p.id).copied().unwrap_or(0.0),
        play_state: match app.launching.borrow().get(&p.id) { Some(false) => 1, Some(true) => 2, None => 0 },
        install_error: app.install_error.borrow().contains(&p.id),
        stats: stats.into(),
        fit_cover: app.fit_cover(&p),
        repo_url: match p.repo.host.as_str() {
            "github" => format!("https://github.com/{}", p.repo.slug()),
            "gitlab" => format!("https://gitlab.com/{}", p.repo.slug()),
            _ => p.direct.as_ref().and_then(|d| d.page.clone()).unwrap_or_default(),
        }
        .into(),
        repo_label: match p.repo.host.as_str() {
            "github" => "↗ Ver en GitHub",
            "gitlab" => "↗ Ver en GitLab",
            _ => "↗ Web oficial",
        }
        .into(),
        last_updated: {
            let cl = app.changelog_cache.borrow();
            let (tag, date) = cl.get(&p.id).map(|(t, d, _)| (t.clone(), d.clone())).unwrap_or_else(|| {
                let c = p.cached.as_ref();
                (
                    c.and_then(|c| c.latest_tag.clone()).unwrap_or_default(),
                    c.and_then(|c| c.published_at.clone()).unwrap_or_default(),
                )
            });
            if date.is_empty() {
                String::new()
            } else if tag.is_empty() {
                format!("Actualizado {}", fmt_date(&date))
            } else {
                format!("{} · {}", tag, fmt_date(&date))
            }
            .into()
        },
        changelog: app.changelog_cache.borrow().get(&p.id).map(|(_, _, b)| b.clone()).unwrap_or_default().into(),
        about: state.wiki.as_ref().map(|w| w.extract.clone()).unwrap_or_default().into(),
        port_notes: p.rom.notes.clone().into(),
        has_related: !related.is_empty(),
        has_versions: versions.len() > 1,
        has_mods: !mod_rows.is_empty(),
        has_screens: !screens.is_empty(),
        ra_eligible: p.ra_supported && freeport_core::ra_mod::platform_supported(),
        ra_enabled: p.ra_supported && freeport_core::ra_mod::is_enabled(&p),
        ra_beta: p.ra_beta,
        kind: if p.kind == "recompilation" { "RECOMP" } else { "PORT" }.into(),
        tier: p.tier().into(),
        aspect: box_aspect(&p.system),
        default_labels: ModelRc::new(VecModel::from(default_labels)),
        default_index,
        version: installed_tag.into(),
        new_version: if update { latest_tag.into() } else { "".into() },
        facts: ModelRc::new(VecModel::from(facts)),
        ra_summary: ra_summary.into(),
        tabs: ModelRc::new(VecModel::from(tabs)),
        sys_logo: app.logos.get(&p.system).cloned().unwrap_or_default(),
        has_sys_logo: app.logos.contains_key(&p.system),
        sys_color,
        status: status.into(),
        status_color,
        add_game_kind: add_kind.into(),
        add_game_label: add_label.into(),
        badge_rows: ModelRc::new(VecModel::from(badge_rows)),
        related: ModelRc::new(VecModel::from(related)),
        versions: ModelRc::new(VecModel::from(versions)),
        mods: ModelRc::new(VecModel::from(mod_rows)),
        screens: ModelRc::new(VecModel::from(screens)),
    };
    win.set_detail_msg(state.msg.clone().into());
    win.set_detail(data);
    win.set_detail_visible(true);
}

/// Home carousel data: consoles that have something to show under the
/// current filters (library / search / experimental), and the games of the
/// console in focus (installed first, then the catalog order).
/// Opens «¿Eliminar…?» for one exact project, listing the port's own data
/// folders (settings, saves) that «borrar también» would remove.
fn open_confirm(app: &App, w: &MainWindow, p: &Project, title: &str) {
    let installed = store::load_installed(&app.paths).unwrap_or_default();
    let others: Vec<Project> = installed.keys().filter(|k| **k != p.id).filter_map(|k| app.find(k)).collect();
    let home = dirs_home();
    let extra: Vec<String> = actions::port_data_dirs(p, &others)
        .iter()
        .map(|d| match d.strip_prefix(&home) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => d.display().to_string(),
        })
        .collect();
    w.set_confirm_id(p.id.clone().into());
    w.set_confirm_title(title.into());
    w.set_confirm_extra(extra.join("\n").into());
    w.set_confirm_wipe(false);
    w.set_confirm_visible(true);
}

fn dirs_home() -> std::path::PathBuf {
    std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default()
}

/// Id of the home carousel's first card: the games played lately.
const RECENT_ID: &str = "recent";

/// Home carousel data: consoles that have something to show under the
/// current filters (library / search / experimental), and the games of the
/// console in focus (installed first, then the catalog order). The first card
/// is «Jugados recientemente» when something has been played.
fn build_home(
    app: &App,
    win: &MainWindow,
    catalog: &Catalog,
    cards: &[(CardItem, String)],
    installed: &store::InstalledMap,
    no_search: bool,
) {
    // Up to three covers per console card (installed first), as small thumbs.
    let peek = |list: &[&CardItem]| -> Vec<CardItem> {
        let mut v: Vec<CardItem> = list.iter().filter(|c| c.installed).take(3).map(|c| (*c).clone()).collect();
        for c in list.iter().filter(|c| !c.installed) {
            if v.len() >= 3 {
                break;
            }
            v.push((*c).clone());
        }
        for c in v.iter_mut() {
            if let Some(p) = app.find(c.id.as_str()) {
                c.cover = app.cover(&p);
                c.fit_cover = fit_for(&c.cover, &p.system);
            }
        }
        v
    };
    let played = |id: &str| -> (i64, u64) {
        installed
            .get(id)
            .map(|e| (e.last_played.as_ref().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0), e.play_secs))
            .unwrap_or((0, 0))
    };
    // «Jugados recientemente»: installed games by last session, newest first.
    let mut recent: Vec<(i64, CardItem)> = cards
        .iter()
        .filter(|(c, _)| c.installed)
        .filter_map(|(c, _)| {
            let (last, secs) = played(c.id.as_str());
            (last > 0).then(|| {
                let mut c = c.clone();
                c.meta = if secs >= 60 {
                    format!("Jugado {} · {} en total", fmt_ago(last), fmt_duration(secs)).into()
                } else {
                    format!("Jugado {}", fmt_ago(last)).into()
                };
                (last, c)
            })
        })
        .collect();
    recent.sort_by_key(|t| std::cmp::Reverse(t.0));
    let recent: Vec<CardItem> = recent.into_iter().take(12).map(|t| t.1).collect();

    let mut systems: Vec<HomeSystem> = Vec::new();
    let mut ids: Vec<String> = Vec::new();
    if no_search && !recent.is_empty() {
        let refs: Vec<&CardItem> = recent.iter().collect();
        systems.push(HomeSystem {
            id: RECENT_ID.into(),
            name: "Jugados recientemente".into(),
            color: Color::from_rgb_u8(0x2f, 0xd3, 0xef),
            logo: Image::default(),
            has_logo: false,
            count: recent.len() as i32,
            installed: recent.len() as i32,
            covers: ModelRc::new(VecModel::from(peek(&refs))),
            is_recent: true,
        });
        ids.push(RECENT_ID.into());
    }
    for s in &catalog.systems {
        let list: Vec<&CardItem> = cards.iter().filter(|(_, sys)| *sys == s.id).map(|(c, _)| c).collect();
        if list.is_empty() {
            continue;
        }
        let inst = list.iter().filter(|c| c.installed).count();
        let logo = app.logos.get(&s.id).cloned();
        systems.push(HomeSystem {
            id: s.id.clone().into(),
            name: s.name.clone().into(),
            color: parse_color(&s.color),
            logo: logo.clone().unwrap_or_default(),
            has_logo: logo.is_some(),
            count: list.len() as i32,
            installed: inst as i32,
            covers: ModelRc::new(VecModel::from(peek(&list))),
            is_recent: false,
        });
        ids.push(s.id.clone());
    }
    let mut sel_id = app.home_sys_id.borrow().clone();
    let sel = ids.iter().position(|i| *i == sel_id).unwrap_or(0);
    if let Some(id) = ids.get(sel) {
        sel_id = id.clone();
        *app.home_sys_id.borrow_mut() = sel_id.clone();
    }
    let color = if sel_id == RECENT_ID {
        Color::from_rgb_u8(0x2f, 0xd3, 0xef)
    } else {
        catalog
            .systems
            .iter()
            .find(|s| s.id == sel_id)
            .map(|s| parse_color(&s.color))
            .unwrap_or(Color::from_rgb_u8(0x3a, 0x3d, 0x46))
    };
    let mut games: Vec<CardItem> = if sel_id == RECENT_ID {
        recent
    } else {
        let mut g: Vec<CardItem> = cards.iter().filter(|(_, sys)| *sys == sel_id).map(|(c, _)| c.clone()).collect();
        g.sort_by_key(|c| !c.installed); // stable: keeps favourites/alphabetical inside each half
        g
    };
    for c in games.iter_mut() {
        if let Some(p) = app.find(c.id.as_str()) {
            c.cover = app.cover_cached(&p);
            c.fit_cover = fit_for(&c.cover, &p.system);
        }
    }
    // Row layout for the home boxes: each one as wide as its console's box
    // (mixed in «Jugados recientemente»), centres in small-box heights.
    let mut x = 0.0f32;
    for c in games.iter_mut() {
        let a = c.aspect.clamp(0.56, 1.45);
        c.cx = x + a / 2.0;
        x += a + 0.1;
    }
    let game_ids: Vec<String> = games.iter().map(|c| c.id.to_string()).collect();
    let gsel = game_ids.iter().position(|i| *i == *app.home_game_id.borrow()).unwrap_or(0);
    if let Some(id) = game_ids.get(gsel) {
        *app.home_game_id.borrow_mut() = id.clone();
    }
    if games.is_empty() && win.get_home_level() == 1 {
        win.set_home_level(0);
    }
    *app.home_sys_ids.borrow_mut() = ids;
    *app.home_game_ids.borrow_mut() = game_ids;
    win.set_home_systems(ModelRc::new(VecModel::from(systems)));
    win.set_home_sys(sel as i32);
    // A peek at the console's games for the console screen: installed first.
    let mut preview: Vec<CardItem> = games.iter().take(7).cloned().collect();
    for c in preview.iter_mut() {
        if let Some(p) = app.find(c.id.as_str()) {
            c.cover = app.cover(&p);
        }
    }
    win.set_home_preview(ModelRc::new(VecModel::from(preview)));
    win.set_home_games(ModelRc::new(VecModel::from(games)));
    win.set_home_game(gsel as i32);
    win.set_home_color(color);
    home_window(app, win);
}

/// One input on the home carousel (keyboard, mouse buttons on it, gamepad).
fn home_key(app: &App, w: &MainWindow, k: &str) {
    let level = w.get_home_level();
    if level == 0 {
        let ids = app.home_sys_ids.borrow().clone();
        if ids.is_empty() {
            return;
        }
        let cur = w.get_home_sys().max(0) as usize;
        let next = match k {
            "left" | "lb" => cur.saturating_sub(1),
            "right" | "rb" => (cur + 1).min(ids.len() - 1),
            "a" | "down" | "start" => {
                w.set_home_level(1);
                w.set_home_btn(-1);
                w.invoke_nav_push();
                home_window(app, w);
                return;
            }
            _ => return,
        };
        if next != cur {
            *app.home_sys_id.borrow_mut() = ids[next].clone();
            app.home_game_id.borrow_mut().clear();
            w.set_home_sys(next as i32);
            ui_refresh();
        }
        return;
    }
    let ids = app.home_game_ids.borrow().clone();
    let cur = w.get_home_game().max(0) as usize;
    // The row of buttons under the game: ▼ enters it, ◀ ▶ move, A presses,
    // ▲ / B go back to the boxes.
    let btn = w.get_home_btn();
    if btn >= 0 {
        let g = w.get_home_games().row_data(cur);
        let has_update = g.as_ref().map(|g| g.installed && g.update_available).unwrap_or(false);
        let n = if has_update { 3 } else { 2 };
        match k {
            // ◀ on the first button goes back to the list.
            "left" | "lb" => w.set_home_btn(btn - 1),
            "right" | "rb" => w.set_home_btn((btn + 1).min(n - 1)),
            "up" | "down" | "b" | "back" => w.set_home_btn(-1),
            "a" | "start" => {
                let action = match (btn, has_update) {
                    (0, _) => "a",
                    (1, true) => "update",
                    _ => "x",
                };
                w.set_home_btn(-1);
                match action {
                    "update" => {
                        if let Some(id) = ids.get(cur) {
                            w.invoke_install(id.clone().into());
                        }
                    }
                    other => home_key(app, w, other),
                }
            }
            _ => {}
        }
        return;
    }
    match k {
        // The list: ▲ ▼ one game (wrapping), LB / RB and Re Pág / Av Pág a page.
        "up" | "down" | "lb" | "rb" | "pgup" | "pgdn" => {
            if ids.is_empty() {
                return;
            }
            let n = ids.len();
            let next = match k {
                "up" => (cur + n - 1) % n,
                "down" => (cur + 1) % n,
                "lb" | "pgup" => cur.saturating_sub(10),
                _ => (cur + 10).min(n - 1),
            };
            *app.home_game_id.borrow_mut() = ids[next].clone();
            w.set_home_game(next as i32);
            home_window(app, w);
        }
        // ▶ into the buttons of the panel.
        "right" => {
            if !ids.is_empty() {
                w.set_home_btn(0);
            }
        }
        "b" | "back" => {
            w.set_home_level(0);
            w.invoke_nav_push();
        }
        "a" | "start" => {
            let Some(id) = ids.get(cur) else { return };
            let installed = store::load_installed(&app.paths).unwrap_or_default();
            if app.busy.borrow().contains(id) {
                return;
            }
            if installed.contains_key(id) {
                w.invoke_play(id.clone().into());
            } else {
                w.invoke_install(id.clone().into());
            }
        }
        "x" | "y" => {
            if let Some(id) = ids.get(cur) {
                w.invoke_open_game(id.clone().into());
            }
        }
        _ => {}
    }
}

/// Shows the «Actualizar todo» batch: the list and the one-line summary.
fn push_bulk(app: &App, w: &MainWindow) {
    let items = app.bulk_items.borrow();
    let rows: Vec<BulkItem> = items
        .iter()
        .map(|(_, t, st, p)| BulkItem { title: t.clone().into(), state: *st, progress: *p })
        .collect();
    let total = items.len();
    let done = items.iter().filter(|i| i.2 >= 2).count();
    let failed = items.iter().filter(|i| i.2 == 3).count();
    let label = if let Some((_, t, _, p)) = items.iter().find(|i| i.2 == 1) {
        let pct = if *p > 0.0 { format!(" · {}%", (p * 100.0).floor()) } else { String::new() };
        format!("Actualizando {} de {total} · {t}{pct}", done + 1)
    } else if w.get_bulk_busy() {
        format!("Actualizando {} de {total}", (done + 1).min(total))
    } else if failed > 0 {
        format!("{} al día · {failed} con error", done - failed)
    } else {
        let ok = items.iter().filter(|i| i.2 == 2).count();
        if ok == 1 { "✓ 1 juego actualizado".to_string() } else { format!("✓ {ok} juegos actualizados") }
    };
    w.set_bulk_items(ModelRc::new(VecModel::from(rows)));
    w.set_bulk_label(label.into());
}

/// Download progress without a rebuild: the bar of the boxes that are busy
/// and the open game page (a full refresh per percent re-created every box).
fn progress_refresh() {
    UI.with(|u| {
        let Some((app, weak)) = &*u.borrow() else { return };
        let Some(w) = weak.upgrade() else { return };
        let busy = app.busy.borrow();
        let prog = app.install_progress.borrow();
        for model in [w.get_home_games(), w.get_home_preview()] {
            for i in 0..model.row_count() {
                let Some(mut row) = model.row_data(i) else { continue };
                let id = row.id.to_string();
                let b = busy.contains(&id);
                let p = prog.get(&id).copied().unwrap_or(0.0);
                if row.busy != b || (row.progress - p).abs() > f32::EPSILON {
                    row.busy = b;
                    row.progress = p;
                    model.set_row_data(i, row);
                }
            }
        }
        drop((busy, prog));
        if w.get_detail_visible() {
            build_detail(app, &w);
        }
    });
}

/// One rebuild for a burst of background events (cover downloads arrive in
/// dozens): coalesced into a single refresh a moment later.
fn schedule_refresh() {
    REFRESH_SOON.with(|t| {
        let mut t = t.borrow_mut();
        if t.as_ref().map(|x| x.running()).unwrap_or(false) {
            return;
        }
        let timer = slint::Timer::default();
        timer.start(slint::TimerMode::SingleShot, std::time::Duration::from_millis(400), ui_refresh);
        *t = Some(timer);
    });
}

/// Rebuild the UI from the current app state (runs on the UI thread).
/// Downloads a chosen RomM entry for `pid` with progress in the ficha, then
/// hands it to the port (copy / import / register).
fn romm_fetch_ui(app: &Rc<App>, pid: &str, rom: freeport_core::romm::Rom) {
    let Some(project) = app.find(pid) else { return };
    let cfg = store::load_config(&app.paths).unwrap_or_default();
    let Some(romm) = freeport_core::romm::Romm::from_config(&cfg) else { return };
    let id = pid.to_string();
    app.busy.borrow_mut().insert(id.clone());
    app.install_progress.borrow_mut().insert(id.clone(), 0.0);
    app.phase.borrow_mut().insert(id.clone(), format!("Descargando {}", rom.fs_name));
    ui_refresh();
    let client = app.client.clone();
    let paths = app.paths.clone();
    let handle = RUNTIME.with(|r| r.borrow().clone());
    let Some(handle) = handle else { return };
    let prog_id = id.clone();
    let on_prog = move |done: u64, total: u64, label: &str| {
        let idc = prog_id.clone();
        let f = if total == 0 { 0.0 } else { done as f32 / total as f32 };
        let text = label.to_string();
        let _ = slint::invoke_from_event_loop(move || {
            UI.with(|u| {
                if let Some((app, _)) = &*u.borrow() {
                    app.install_progress.borrow_mut().insert(idc.clone(), f);
                    app.phase.borrow_mut().insert(idc, text);
                }
            });
            progress_refresh();
        });
    };
    let pid2 = id.clone();
    handle.spawn(async move {
        let res = actions::romm_fetch(&client, &paths, &project, &romm, &rom, on_prog).await;
        let _ = slint::invoke_from_event_loop(move || {
            UI.with(|u| {
                if let Some((app, weak)) = &*u.borrow() {
                    app.busy.borrow_mut().remove(&pid2);
                    app.install_progress.borrow_mut().remove(&pid2);
                    app.phase.borrow_mut().remove(&pid2);
                    if let Some(w) = weak.upgrade() {
                        match &res {
                            Ok(p) => w.set_notice(format!("ROM lista: {}", p.display()).into()),
                            Err(e) => w.set_notice(format!("RomM: {e}").into()),
                        }
                    }
                }
            });
            ui_refresh();
        });
    });
}

fn ui_refresh() {
    UI.with(|u| {
        if let Some((app, weak)) = &*u.borrow() {
            if let Some(w) = weak.upgrade() {
                let t0 = std::time::Instant::now();
                rebuild(app, &w);
                build_detail(app, &w);
                if std::env::var_os("FREEPORT_DEBUG_TIMING").is_some() {
                    eprintln!("[timing] ui_refresh {:.1} ms", t0.elapsed().as_secs_f64() * 1000.0);
                }
            }
        }
    });
}

/// Runs a self-update check and reflects the result in the UI. `manual` = the
/// user pressed "Buscar actualizaciones" (show up-to-date/error feedback and
/// ignore a previously skipped version); otherwise it's an automatic check.
async fn run_update_check(client: reqwest::Client, manual: bool) {
    let res = update::check(&client, env!("CARGO_PKG_VERSION")).await;
    let _ = slint::invoke_from_event_loop(move || {
        UI.with(|u| {
            let Some((app, weak)) = &*u.borrow() else { return };
            let Some(w) = weak.upgrade() else { return };
            match res {
                Ok(Some(upd)) => {
                    let skipped = store::load_config(&app.paths).ok().and_then(|c| c.skip_version);
                    if !manual && skipped.as_deref() == Some(upd.version.as_str()) {
                        return; // automatic check respects a skipped version
                    }
                    w.set_update_version(upd.version.clone().into());
                    w.set_update_notes(upd.notes.clone().into());
                    w.set_update_error(false);
                    w.set_update_available(true);
                    if manual {
                        w.set_settings_msg(format!("Nueva versión v{} disponible ↑", upd.version).into());
                    }
                    *app.pending_update.borrow_mut() = Some(upd);
                }
                Ok(None) => {
                    if manual {
                        w.set_settings_msg("Ya estás en la última versión ✓".into());
                    }
                }
                Err(_) => {
                    if manual {
                        w.set_settings_msg("No se pudo comprobar".into());
                    }
                }
            }
        });
    });
}

/// Generate any not-yet-cached cover thumbnails in the background, then refresh.
fn spawn_missing_thumbs(app: &Rc<App>, handle: &tokio::runtime::Handle) {
    let missing: Vec<String> = app
        .catalog
        .borrow()
        .projects
        .iter()
        .filter_map(|p| p.box_art.clone().or_else(|| p.cover_url.clone()))
        .filter(|u| !thumbs::path_for(&app.paths, u).exists())
        .collect();
    if missing.is_empty() {
        return;
    }
    let client = app.client.clone();
    let paths = app.paths.clone();
    handle.spawn(async move {
        for u in missing {
            let _ = thumbs::get(&client, &paths, &u).await;
        }
        let _ = slint::invoke_from_event_loop(ui_refresh);
    });
}

/// Headless `freeport --play <id>`: launch a game and stay alive while it runs
/// (lets Steam non-Steam shortcuts track the session).
fn cli_play(id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let paths = Paths::resolve()?;
    let catalog = store::load_catalog(&paths)?;
    let project = catalog
        .projects
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("juego desconocido: {id}"))?
        .clone();
    let pid = actions::launch_project(&paths, &project).map_err(|e| e.to_string())?;
    println!("[freeport] jugando {} (pid {pid})", project.name);
    while Path::new(&format!("/proc/{pid}")).exists() {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    Ok(())
}

/// Recomputes the total on-disk size of the library and updates the sidebar.
async fn refresh_library_size(paths: Paths) {
    let sizes: std::collections::HashMap<String, u64> = tokio::task::spawn_blocking(move || {
        let installed = store::load_installed(&paths).unwrap_or_default();
        installed.iter().map(|(id, e)| (id.clone(), dir_size(Path::new(&e.install_path)))).collect()
    })
    .await
    .unwrap_or_default();
    let total: u64 = sizes.values().sum();
    let _ = slint::invoke_from_event_loop(move || {
        UI.with(|u| {
            if let Some((app, weak)) = &*u.borrow() {
                *app.size_cache.borrow_mut() = sizes;
                if let Some(w) = weak.upgrade() {
                    w.set_library_size(if total > 0 { fmt_bytes(total).into() } else { "".into() });
                }
            }
        });
        ui_refresh();
    });
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--list-data-dirs") {
        // dev aid: what «borrar también sus partidas y ajustes» would offer for
        // each installed game (prints only, removes nothing).
        let paths = Paths::resolve()?;
        let catalog = store::load_catalog(&paths)?;
        let installed = store::load_installed(&paths)?;
        let inst: Vec<Project> = catalog.projects.iter().filter(|p| installed.contains_key(&p.id)).cloned().collect();
        for p in &inst {
            println!("{}: {:?}", p.id, actions::port_data_dirs(p, &inst));
        }
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--play") {
        let id = args.get(i + 1).cloned().unwrap_or_default();
        return cli_play(&id);
    }
    // Pin the winit backend (the one every platform ships with).
    let backend = i_slint_backend_winit::Backend::new().expect("winit backend");
    slint::platform::set_platform(Box::new(backend)).expect("set winit platform");
    // Wayland app_id / X11 WM_CLASS = the .desktop file's name, so the taskbar
    // and Alt+Tab find freeport.desktop and show its icon.
    let _ = slint::set_xdg_app_id("freeport");

    // Bundle DejaVu Sans so UI glyphs (arrows, ★, ⚓, ⚙…) render identically on
    // every OS instead of falling back to each system's fonts (Windows showed
    // some as color emoji or missing). Registered as the default font family.
    {
        use slint::fontique_010::{fontique, shared_collection};
        let mut c = shared_collection();
        c.register_fonts(
            fontique::Blob::new(std::sync::Arc::new(
                include_bytes!("../assets/fonts/DejaVuSans.ttf").to_vec(),
            )),
            None,
        );
        c.register_fonts(
            fontique::Blob::new(std::sync::Arc::new(
                include_bytes!("../assets/fonts/DejaVuSans-Bold.ttf").to_vec(),
            )),
            None,
        );
        // Nunito (OFL, rounded): the UI's typeface. DejaVu above stays as the
        // fallback for symbols Nunito lacks (▶ ✓ ✕ ⚙…).
        for bytes in [
            &include_bytes!("../assets/fonts/Nunito-Regular.ttf")[..],
            &include_bytes!("../assets/fonts/Nunito-SemiBold.ttf")[..],
            &include_bytes!("../assets/fonts/Nunito-Bold.ttf")[..],
            &include_bytes!("../assets/fonts/Nunito-ExtraBold.ttf")[..],
            &include_bytes!("../assets/fonts/Nunito-Black.ttf")[..],
        ] {
            c.register_fonts(fontique::Blob::new(std::sync::Arc::new(bytes.to_vec())), None);
        }
    }

    update::cleanup();
    let rt = tokio::runtime::Runtime::new()?;
    let handle = rt.handle().clone();

    let paths = Paths::resolve()?;
    let catalog = store::load_catalog(&paths)?;
    let logos = load_logos(&paths.data_dir.join("native_logos"));
    let client = reqwest::Client::builder().user_agent("freeport").build()?;

    let app = Rc::new(App {
        catalog: RefCell::new(catalog),
        triple: platform::current_triple(),
        paths,
        client,
        logos,
        runners: actions::list_runners(),
        busy: RefCell::new(HashSet::new()),
        install_progress: RefCell::new(std::collections::HashMap::new()),
        phase: RefCell::new(std::collections::HashMap::new()),
        romm_candidates: RefCell::new(Vec::new()),
        romm_project: RefCell::new(String::new()),
        detail: RefCell::new(None),
        mods_cache: RefCell::new(std::collections::HashMap::new()),
        mod_busy: RefCell::new(HashSet::new()),
        mod_progress: RefCell::new(std::collections::HashMap::new()),
        mod_icons: RefCell::new(std::collections::HashMap::new()),
        screens_cache: RefCell::new(std::collections::HashMap::new()),
        cover_cache: RefCell::new(std::collections::HashMap::new()),
        cover_hd: RefCell::new(std::collections::HashMap::new()),
        bulk_items: RefCell::new(Vec::new()),
        img_pending: RefCell::new(HashSet::new()),
        hd_wanted: RefCell::new(HashSet::new()),
        media: RefCell::new(std::collections::HashMap::new()),
        info_desc: RefCell::new(std::collections::HashMap::new()),
        info_id: RefCell::new(String::new()),
        size_cache: RefCell::new(std::collections::HashMap::new()),
        changelog_cache: RefCell::new(std::collections::HashMap::new()),
        pending_update: RefCell::new(None),
        launching: RefCell::new(std::collections::HashMap::new()),
        exp_systems: RefCell::new(HashSet::new()),
        exp_all: std::cell::Cell::new(false),
        chooser_mode: RefCell::new(String::new()),
        install_error: RefCell::new(HashSet::new()),
        home_sys_id: RefCell::new(String::new()),
        home_game_id: RefCell::new(String::new()),
        home_sys_ids: RefCell::new(Vec::new()),
        home_game_ids: RefCell::new(Vec::new()),
        nav_hist: RefCell::new(Vec::new()),
        nav_pos: std::cell::Cell::new(0),
        nav_applying: std::cell::Cell::new(false),
        seen: RefCell::new(std::collections::HashMap::new()),
        ra_cache: RefCell::new(std::collections::HashMap::new()),
        ra_badges: RefCell::new(std::collections::HashMap::new()),
        cancels: RefCell::new(std::collections::HashMap::new()),
        bulk_cancel: RefCell::new(None),
    });

    let win = MainWindow::new()?;
    UI.with(|u| *u.borrow_mut() = Some((app.clone(), win.as_weak())));

    // Controller input → route to TV navigation when Big Picture is open.
    // FREEPORT_NO_GAMEPAD: headless/test instances never read the controller.
    if std::env::var_os("FREEPORT_NO_GAMEPAD").is_none() { gamepad::start(|b| {
        if !WINDOW_FOCUSED.load(std::sync::atomic::Ordering::Relaxed) {
            return; // another window (a game) has the player's attention
        }
        let b = b.to_string();
        let _ = slint::invoke_from_event_loop(move || {
            UI.with(|u| {
                if let Some((app, weak)) = &*u.borrow() {
                    if let Some(w) = weak.upgrade() {
                        // Whatever is on top gets the button: dialogs, then the
                        // game page / settings, then the home carousel.
                        if w.get_confirm_visible() || w.get_chooser_visible() {
                            w.invoke_dialog_key(b.clone().into());
                        } else if w.get_settings_visible() {
                            w.invoke_settings_key(b.clone().into());
                        } else if w.get_detail_visible() {
                            w.invoke_detail_key(b.clone().into());
                        } else {
                            home_key(app, &w, &b);
                        }
                    }
                }
            });
        });
    }); }


    // Populate settings UI.
    {
        let cfg = store::load_config(&app.paths).unwrap_or_default();
        win.set_version(env!("CARGO_PKG_VERSION").into());
        win.set_platform_label(app.triple.clone().into());
        win.set_show_windows(cfg.show_windows);
        win.set_crt_visible(cfg.crt);
        if let (Some(u), Some(_)) = (cfg.ra_user.as_ref(), cfg.ra_token.as_ref()) {
            win.set_ra_logged_in(true);
            win.set_ra_status(format!("Conectado como {u}").into());
        }
        let mut labels: Vec<SharedString> = vec!["Automático".into()];
        for r in &app.runners {
            labels.push(r.label.clone().into());
        }
        win.set_runner_labels(ModelRc::new(VecModel::from(labels.clone())));
        let current = cfg
            .wine_runner
            .as_ref()
            .and_then(|id| app.runners.iter().find(|r| &r.id == id))
            .map(|r| r.label.clone())
            .unwrap_or_else(|| "Automático".to_string());
        let idx = labels.iter().position(|l| l.as_str() == current).unwrap_or(0);
        win.set_current_runner_label(current.into());
        win.set_runner_index(idx as i32);
    }

    win.on_toggle_windows({
        let app = app.clone();
        move |v| {
            if let Ok(mut c) = store::load_config(&app.paths) {
                c.show_windows = v;
                let _ = store::save_config(&app.paths, &c);
            }
            ui_refresh();
        }
    });

    win.on_toggle_experimental_system({
        let app = app.clone();
        move |id, on| {
            if let Ok(mut c) = store::load_config(&app.paths) {
                let id = id.to_string();
                // Leaving the legacy global switch on would override per-system choices.
                if c.show_experimental && c.experimental_systems.is_empty() {
                    c.experimental_systems = app.catalog.borrow().systems.iter().map(|s| s.id.clone()).collect();
                    c.show_experimental = false;
                }
                c.experimental_systems.retain(|s| s != &id);
                if on {
                    c.experimental_systems.push(id);
                }
                let _ = store::save_config(&app.paths, &c);
            }
            ui_refresh();
        }
    });

    win.on_set_all_experimental({
        let app = app.clone();
        move |on| {
            if let Ok(mut c) = store::load_config(&app.paths) {
                c.show_experimental = false;
                c.experimental_systems = if on {
                    app.catalog.borrow().systems.iter().map(|s| s.id.clone()).collect()
                } else {
                    Vec::new()
                };
                let _ = store::save_config(&app.paths, &c);
            }
            ui_refresh();
        }
    });

    win.on_toggle_crt({
        let app = app.clone();
        move |v| {
            if let Ok(mut c) = store::load_config(&app.paths) {
                c.crt = v;
                let _ = store::save_config(&app.paths, &c);
            }
        }
    });

    win.on_ra_login({
        let app = app.clone();
        let handle = handle.clone();
        let weak = win.as_weak();
        move |user, pass| {
            if user.is_empty() || pass.is_empty() {
                if let Some(w) = weak.upgrade() { w.set_ra_status("Introduce usuario y contraseña".into()); }
                return;
            }
            if let Some(w) = weak.upgrade() { w.set_ra_status("Conectando…".into()); }
            let client = app.client.clone();
            let user = user.to_string();
            let pass = pass.to_string();
            handle.spawn(async move {
                let res = ra::login(&client, &user, &pass).await;
                let _ = slint::invoke_from_event_loop(move || {
                    UI.with(|u| {
                        if let Some((app, weak)) = &*u.borrow() {
                            let Some(w) = weak.upgrade() else { return };
                            match res {
                                Ok((ruser, token)) => {
                                    if let Ok(mut c) = store::load_config(&app.paths) {
                                        c.ra_user = Some(ruser.clone());
                                        c.ra_token = Some(token);
                                        let _ = store::save_config(&app.paths, &c);
                                    }
                                    w.set_ra_logged_in(true);
                                    w.set_ra_status(format!("Conectado como {ruser}").into());
                                }
                                Err(e) => {
                                    w.set_ra_logged_in(false);
                                    w.set_ra_status(e.into());
                                }
                            }
                        }
                    });
                });
            });
        }
    });

    win.on_ra_logout({
        let app = app.clone();
        let weak = win.as_weak();
        move || {
            if let Ok(mut c) = store::load_config(&app.paths) {
                c.ra_user = None;
                c.ra_token = None;
                let _ = store::save_config(&app.paths, &c);
            }
            if let Some(w) = weak.upgrade() {
                w.set_ra_logged_in(false);
                w.set_ra_status("".into());
            }
        }
    });

    RUNTIME.with(|r| *r.borrow_mut() = Some(handle.clone()));
    {
        let cfg = store::load_config(&app.paths).unwrap_or_default();
        win.set_romm_url(cfg.romm_url.clone().unwrap_or_default().into());
        win.set_romm_user(cfg.romm_user.clone().unwrap_or_default().into());
        if let Some(r) = freeport_core::romm::Romm::from_config(&cfg) {
            win.set_romm_connected(true);
            win.set_romm_status(format!("Conectado a {}", r.url).into());
        }
    }
    win.on_romm_connect({
        let app = app.clone();
        let handle = handle.clone();
        let weak = win.as_weak();
        move |url, user, pass| {
            let url = url.trim().trim_end_matches('/').to_string();
            if url.is_empty() {
                if let Some(w) = weak.upgrade() { w.set_romm_status("Indica la URL de tu RomM".into()); }
                return;
            }
            if let Some(w) = weak.upgrade() { w.set_romm_status("Conectando…".into()); }
            let romm = freeport_core::romm::Romm { url: url.clone(), user: user.to_string(), pass: pass.to_string() };
            let client = app.client.clone();
            handle.spawn(async move {
                let res = freeport_core::romm::probe(&client, &romm).await;
                let _ = slint::invoke_from_event_loop(move || {
                    UI.with(|u| {
                        if let Some((app, weak)) = &*u.borrow() {
                            let Some(w) = weak.upgrade() else { return };
                            match res {
                                Ok((version, n)) => {
                                    if let Ok(mut c) = store::load_config(&app.paths) {
                                        c.romm_url = Some(romm.url.clone());
                                        c.romm_user = Some(romm.user.clone());
                                        c.romm_pass = Some(romm.pass.clone());
                                        let _ = store::save_config(&app.paths, &c);
                                    }
                                    w.set_romm_connected(true);
                                    w.set_romm_status(format!("Conectado a {} (RomM {version}, {n} sistemas con juegos)", romm.url).into());
                                    ui_refresh();
                                }
                                Err(e) => {
                                    w.set_romm_connected(false);
                                    w.set_romm_status(e.to_string().into());
                                }
                            }
                        }
                    });
                });
            });
        }
    });
    win.on_romm_forget({
        let app = app.clone();
        let weak = win.as_weak();
        move || {
            if let Ok(mut c) = store::load_config(&app.paths) {
                c.romm_url = None;
                c.romm_user = None;
                c.romm_pass = None;
                let _ = store::save_config(&app.paths, &c);
            }
            if let Some(w) = weak.upgrade() {
                w.set_romm_connected(false);
                w.set_romm_status("".into());
            }
            ui_refresh();
        }
    });
    win.on_romm_search({
        let app = app.clone();
        let handle = handle.clone();
        let weak = win.as_weak();
        move |id| {
            let id = id.to_string();
            let Some(project) = app.find(&id) else { return };
            let cfg = store::load_config(&app.paths).unwrap_or_default();
            let Some(romm) = freeport_core::romm::Romm::from_config(&cfg) else { return };
            if let Some(w) = weak.upgrade() { w.set_notice(format!("Buscando «{}» en RomM…", project.original_game).into()); }
            let client = app.client.clone();
            let pid = id.clone();
            handle.spawn(async move {
                let res = actions::romm_candidates(&client, &romm, &project).await;
                let _ = slint::invoke_from_event_loop(move || {
                    UI.with(|u| {
                        if let Some((app, weak)) = &*u.borrow() {
                            let Some(w) = weak.upgrade() else { return };
                            match res {
                                Ok(list) if list.is_empty() => {
                                    w.set_notice(format!("RomM no tiene nada parecido a «{}» en ese sistema", project.original_game).into());
                                }
                                Ok(list) => {
                                    w.set_notice("".into());
                                    let rows: Vec<VersionRow> = list
                                        .iter()
                                        .take(12)
                                        .map(|r| VersionRow {
                                            id: format!("romm:{}", r.id).into(),
                                            name: r.name.clone().into(),
                                            kind: "ROMM".into(),
                                            is_windows: false,
                                            installed: false,
                                            active: false,
                                            platforms: r.fs_name.clone().into(),
                                            meta: format!("{:.1} MB", r.fs_size_bytes as f64 / 1048576.0).into(),
                                        })
                                        .collect();
                                    *app.romm_candidates.borrow_mut() = list;
                                    *app.romm_project.borrow_mut() = pid.clone();
                                    open_chooser(app, &w, "romm", &format!("¿Cuál es {}?", project.original_game), rows);
                                }
                                Err(e) => w.set_notice(format!("RomM: {e}").into()),
                            }
                        }
                    });
                });
            });
        }
    });
    if let Some(id) = std::env::var("FREEPORT_DEBUG_ROMM").ok().filter(|s| !s.is_empty()) {
        // dev aid: FREEPORT_DEBUG_ROMM=<project id> searches RomM and fetches the best match.
        if let Some(project) = app.find(&id) {
            let cfg = store::load_config(&app.paths).unwrap_or_default();
            if let Some(romm) = freeport_core::romm::Romm::from_config(&cfg) {
                let client = app.client.clone();
                handle.spawn(async move {
                    let res = actions::romm_candidates(&client, &romm, &project).await;
                    let _ = slint::invoke_from_event_loop(move || {
                        match res {
                            Ok(list) => {
                                for r in list.iter().take(5) {
                                    eprintln!("[romm] candidato {} «{}» {} ({} MB) score {:.2}", r.id, r.name, r.fs_name, r.fs_size_bytes / 1048576, freeport_core::romm::score(r, &project.original_game));
                                }
                                if let Some(best) = list.first().cloned() {
                                    UI.with(|u| {
                                        if let Some((app, _)) = &*u.borrow() {
                                            *app.romm_candidates.borrow_mut() = list;
                                            romm_fetch_ui(app, &project.id, best);
                                        }
                                    });
                                }
                            }
                            Err(e) => eprintln!("[romm] error: {e}"),
                        }
                    });
                });
            } else {
                eprintln!("[romm] sin configurar (romm_url)");
            }
        }
    }

    win.on_toggle_ra({
        let app = app.clone();
        move |id, enable| {
            let Some(project) = app.find(&id) else { return };
            match freeport_core::ra_mod::set_enabled(&project, enable) {
                Ok(()) => ui_refresh(),
                Err(e) => eprintln!("[freeport] toggle RA falló: {e}"),
            }
        }
    });

    win.on_set_runner({
        let app = app.clone();
        move |label| {
            let id = if label.as_str() == "Automático" {
                None
            } else {
                app.runners.iter().find(|r| r.label == label.as_str()).map(|r| r.id.clone())
            };
            if let Ok(mut c) = store::load_config(&app.paths) {
                c.wine_runner = id;
                let _ = store::save_config(&app.paths, &c);
            }
        }
    });

    win.on_refresh_catalog({
        let app = app.clone();
        let handle = handle.clone();
        let weak = win.as_weak();
        move || {
            if let Some(w) = weak.upgrade() {
                if w.get_catalog_busy() {
                    return;
                }
                w.set_catalog_busy(true);
                w.set_settings_msg("".into());
            }
            let client = app.client.clone();
            let paths = app.paths.clone();
            let url = store::load_config(&paths).ok().and_then(|c| c.catalog_url);
            let handle2 = handle.clone();
            handle.spawn(async move {
                let res = actions::refresh_catalog(&client, &paths, url.as_deref())
                    .await
                    .map_err(|e| e.to_string());
                let _ = slint::invoke_from_event_loop(move || {
                    UI.with(|u| {
                        if let Some((app, weak)) = &*u.borrow() {
                            let msg = match res {
                                Ok(cat) => {
                                    *app.catalog.borrow_mut() = cat;
                                    // Record the refreshed ids so NUEVO badges expire
                                    // next session, not never.
                                    let ids: Vec<String> =
                                        app.catalog.borrow().projects.iter().map(|p| p.id.clone()).collect();
                                    save_seen(&app.paths, &ids);
                                    *app.seen.borrow_mut() = load_seen(&app.paths);
                                    "Catálogo actualizado ✓"
                                }
                                Err(_) => "No se pudo actualizar",
                            };
                            if let Some(w) = weak.upgrade() {
                                w.set_settings_msg(msg.into());
                                w.set_catalog_busy(false);
                            }
                        }
                    });
                    ui_refresh();
                    UI.with(|u| {
                        if let Some((app, _)) = &*u.borrow() {
                            spawn_missing_thumbs(app, &handle2);
                        }
                    });
                });
            });
        }
    });

    win.on_home_key({
        let app = app.clone();
        let weak = win.as_weak();
        move |k| {
            if let Some(w) = weak.upgrade() {
                home_key(&app, &w, k.as_str());
            }
        }
    });
    win.on_home_pick_system({
        let app = app.clone();
        let weak = win.as_weak();
        move |i| {
            let Some(w) = weak.upgrade() else { return };
            if i == w.get_home_sys() {
                home_key(&app, &w, "a");
                return;
            }
            if let Some(id) = app.home_sys_ids.borrow().get(i as usize) {
                *app.home_sys_id.borrow_mut() = id.clone();
            }
            app.home_game_id.borrow_mut().clear();
            w.set_home_sys(i);
            ui_refresh();
        }
    });
    win.on_home_pick_game({
        let app = app.clone();
        let weak = win.as_weak();
        move |i| {
            let Some(w) = weak.upgrade() else { return };
            // Click on the box already in focus: its page (with Jugar/Descargar
            // on it). Never the action itself: a double click on a box would
            // start a multi-GB download or launch the game.
            if i == w.get_home_game() {
                home_key(&app, &w, "x");
                return;
            }
            if let Some(id) = app.home_game_ids.borrow().get(i as usize) {
                *app.home_game_id.borrow_mut() = id.clone();
            }
            w.set_home_game(i);
            home_window(&app, &w);
        }
    });
    // A cover in the console's preview strip: into the console, on that game.
    win.on_home_pick_preview({
        let app = app.clone();
        let weak = win.as_weak();
        move |i| {
            let Some(w) = weak.upgrade() else { return };
            if let Some(id) = app.home_game_ids.borrow().get(i as usize) {
                *app.home_game_id.borrow_mut() = id.clone();
            }
            w.set_home_game(i);
            w.set_home_level(1);
            w.set_home_btn(-1);
            w.invoke_nav_push();
            home_window(&app, &w);
        }
    });
    win.on_toggle_fullscreen({
        let weak = win.as_weak();
        move |v| {
            if let Some(w) = weak.upgrade() {
                w.set_fullscreen(v);
                w.window().set_fullscreen(v);
            }
        }
    });
    win.on_refresh(ui_refresh);

    win.on_toggle_favorite({
        let app = app.clone();
        move |id| {
            let id = id.to_string();
            if let Ok(mut cfg) = store::load_config(&app.paths) {
                if let Some(pos) = cfg.favorites.iter().position(|f| f == &id) {
                    cfg.favorites.remove(pos);
                } else {
                    cfg.favorites.push(id.clone());
                }
                let _ = store::save_config(&app.paths, &cfg);
            }
            ui_refresh();
        }
    });

    win.on_open_folder({
        let app = app.clone();
        move |id| {
            if let Some(dir) = actions::installed_dir(&app.paths, &id) {
                let _ = open::that(dir);
            }
        }
    });

    win.on_open_game({
        let app = app.clone();
        let handle = handle.clone();
        let weak = win.as_weak();
        move |id| {
            let id = id.to_string();
            let Some(project) = app.find(&id) else { return };
            let same = app.detail.borrow().as_ref().map(|d| d.id == id).unwrap_or(false);
            if !same {
                if let Some(w) = weak.upgrade() {
                    w.set_detail_tab(0);
                }
            }
            *app.detail.borrow_mut() = Some(DetailState { id: id.clone(), wiki: None, msg: String::new() });
            ui_refresh();
            if let Some(w) = weak.upgrade() {
                w.invoke_nav_push();
            }
            // Fetch the latest release changelog + date (once, cached).
            if !app.changelog_cache.borrow().contains_key(&id) {
                let client = app.client.clone();
                let paths = app.paths.clone();
                let proj = project.clone();
                let pid = id.clone();
                handle.spawn(async move {
                    if let Ok((tag, date, body)) = actions::fetch_changelog(&client, &paths, &proj).await {
                        let _ = slint::invoke_from_event_loop(move || {
                            UI.with(|u| {
                                if let Some((app, _)) = &*u.borrow() {
                                    app.changelog_cache.borrow_mut().insert(pid, (tag, date.unwrap_or_default(), body));
                                }
                            });
                            ui_refresh();
                        });
                    }
                });
            }
            // RetroAchievements progress (RA-supported game + stored session).
            if project.ra_supported && !app.ra_cache.borrow().contains_key(&id) {
                let cfg = store::load_config(&app.paths).unwrap_or_default();
                if let (Some(user), Some(token), Some(rom)) =
                    (cfg.ra_user.clone(), cfg.ra_token.clone(), freeport_core::ra_mod::rom_path(&project))
                {
                    let client = app.client.clone();
                    let paths = app.paths.clone();
                    let pid = id.clone();
                    handle.spawn(async move {
                        let game_id = match ra::game_id_for_rom(&client, Path::new(&rom)).await {
                            Ok(g) => g,
                            Err(e) => { eprintln!("[ra] gameid: {e}"); return }
                        };
                        let progress = match ra::fetch_progress(&client, &user, &token, game_id).await {
                            Ok(p) => p,
                            Err(e) => { eprintln!("[ra] progress: {e}"); return }
                        };
                        eprintln!("[ra] {} logros para game {game_id}", progress.total);
                        // Badge art: cache to disk, then hand paths to the UI thread.
                        let mut fetched: Vec<(String, String)> = Vec::new();
                        for a in &progress.achievements {
                            if a.badge_name.is_empty() {
                                continue;
                            }
                            let url = ra::badge_url(&a.badge_name, a.unlocked);
                            if let Ok(p) = thumbs::get_full(&client, &paths, &url).await {
                                fetched.push((url, p.display().to_string()));
                            }
                        }
                        let _ = slint::invoke_from_event_loop(move || {
                            UI.with(|u| {
                                if let Some((app, _)) = &*u.borrow() {
                                    for (url, path) in &fetched {
                                        if !app.ra_badges.borrow().contains_key(url) {
                                            if let Ok(img) = Image::load_from_path(Path::new(path)) {
                                                app.ra_badges.borrow_mut().insert(url.clone(), img);
                                            }
                                        }
                                    }
                                    app.ra_cache.borrow_mut().insert(pid, progress);
                                }
                            });
                            ui_refresh();
                        });
                    });
                }
            }
            if let Some(title) = project.wiki.clone() {
                let client = app.client.clone();
                let paths = app.paths.clone();
                let want = id.clone();
                handle.spawn(async move {
                    if let Some(w) = wiki::fetch(&client, &paths, &title).await {
                        let _ = slint::invoke_from_event_loop(move || {
                            UI.with(|u| {
                                if let Some((app, _)) = &*u.borrow() {
                                    if let Some(st) = app.detail.borrow_mut().as_mut() {
                                        if st.id == want {
                                            st.wiki = Some(w);
                                        }
                                    }
                                }
                            });
                            ui_refresh();
                        });
                    }
                });
            }
            // Fetch the mod list for this game (once), then refresh the detail.
            if project.mods.is_some() && !app.mods_cache.borrow().contains_key(&id) {
                let client = app.client.clone();
                let paths = app.paths.clone();
                let proj = project.clone();
                let pid = id.clone();
                handle.spawn(async move {
                    if let Ok(list) = actions::list_mods(&client, &proj).await {
                        // Icons to fetch (top of the list keeps it cheap).
                        let icon_urls: Vec<String> =
                            list.iter().filter_map(|m| m.icon_url.clone()).take(48).collect();
                        let listc = list.clone();
                        let pid2 = pid.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            UI.with(|u| {
                                if let Some((app, _)) = &*u.borrow() {
                                    app.mods_cache.borrow_mut().insert(pid2, listc);
                                }
                            });
                            ui_refresh();
                        });
                        // Download mod icons in the background, then load + refresh.
                        let mut fetched: Vec<(String, String)> = Vec::new();
                        for u in icon_urls {
                            if let Ok(p) = thumbs::get_full(&client, &paths, &u).await {
                                fetched.push((u, p.display().to_string()));
                            }
                        }
                        if !fetched.is_empty() {
                            let _ = slint::invoke_from_event_loop(move || {
                                UI.with(|u| {
                                    if let Some((app, _)) = &*u.borrow() {
                                        for (url, path) in &fetched {
                                            if app.mod_icons.borrow().contains_key(url) {
                                                continue;
                                            }
                                            if let Ok(img) = Image::load_from_path(Path::new(path)) {
                                                app.mod_icons.borrow_mut().insert(url.clone(), img);
                                            }
                                        }
                                    }
                                });
                                ui_refresh();
                            });
                        }
                    }
                });
            }
            // Screenshots (libretro snap + title) → cache + refresh.
            if let Some(cover) = project.cover_url.clone() {
                if !app.screens_cache.borrow().contains_key(&id) {
                    let urls = thumbs::screenshot_urls(&cover);
                    if !urls.is_empty() {
                        let client = app.client.clone();
                        let paths = app.paths.clone();
                        let pid = id.clone();
                        handle.spawn(async move {
                            let mut out = Vec::new();
                            for u in urls {
                                if let Ok(p) = thumbs::get_full(&client, &paths, &u).await {
                                    out.push(p.display().to_string());
                                }
                            }
                            if !out.is_empty() {
                                let _ = slint::invoke_from_event_loop(move || {
                                    UI.with(|u| {
                                        if let Some((app, _)) = &*u.borrow() {
                                            app.screens_cache.borrow_mut().insert(pid, out);
                                        }
                                    });
                                    ui_refresh();
                                });
                            }
                        });
                    }
                }
            }
        }
    });

    win.on_open_url(|url| {
        let _ = open::that(url.as_str());
    });

    win.on_install_mod({
        let app = app.clone();
        let handle = handle.clone();
        move |id, full_name| {
            let id = id.to_string();
            let fname = full_name.to_string();
            let Some(project) = app.find(&id) else { return };
            let all = app.mods_cache.borrow().get(&id).cloned().unwrap_or_default();
            app.mod_busy.borrow_mut().insert(fname.clone());
            app.mod_progress.borrow_mut().insert(fname.clone(), (0.0, "download".into()));
            ui_refresh();
            let client = app.client.clone();
            let paths = app.paths.clone();
            let fname2 = fname.clone();
            handle.spawn(async move {
                let prog_key = fname2.clone();
                let mut last = -1i32;
                let on_prog = move |_pkg: &str, i: usize, total: usize, done: u64, bytes: u64, phase: &str| {
                    let frac = if bytes > 0 { done as f64 / bytes as f64 } else { 0.0 };
                    let overall = if total > 0 {
                        (i.saturating_sub(1) as f64 + frac) / total as f64
                    } else {
                        0.0
                    };
                    let pct = (overall * 100.0) as i32;
                    if pct == last && phase == "download" {
                        return; // throttle only the noisy download phase
                    }
                    last = pct;
                    let key = prog_key.clone();
                    let ph = phase.to_string();
                    let f = overall as f32;
                    let _ = slint::invoke_from_event_loop(move || {
                        UI.with(|u| {
                            if let Some((app, _)) = &*u.borrow() {
                                app.mod_progress.borrow_mut().insert(key, (f, ph));
                            }
                        });
                        ui_refresh();
                    });
                };
                let err = actions::install_mod(&client, &paths, &project, &all, &fname2, on_prog).await.err();
                let _ = slint::invoke_from_event_loop(move || {
                    let _ = &err;
                    UI.with(|u| {
                        if let Some((app, _)) = &*u.borrow() {
                            app.mod_busy.borrow_mut().remove(&fname2);
                            app.mod_progress.borrow_mut().remove(&fname2);
                        }
                    });
                    ui_refresh();
                });
            });
        }
    });

    win.on_uninstall_mod({
        let app = app.clone();
        move |id, full_name| {
            if let Some(project) = app.find(&id) {
                let _ = actions::uninstall_mod(&app.paths, &project, &full_name);
            }
            ui_refresh();
        }
    });

    win.on_close_detail({
        let app = app.clone();
        let weak = win.as_weak();
        move || {
            *app.detail.borrow_mut() = None;
            ui_refresh();
            if let Some(w) = weak.upgrade() {
                w.invoke_nav_push();
            }
        }
    });

    // ── View history: mouse back/forward (and the sidebar's own navigation) ──
    let nav_current = |app: &App, w: &MainWindow| NavState {
        home_level: w.get_home_level(),
        settings: w.get_settings_visible(),
        library: w.get_library_tab(),
        detail: app.detail.borrow().as_ref().map(|d| d.id.clone()),
    };
    win.on_nav_push({
        let app = app.clone();
        let weak = win.as_weak();
        move || {
            if app.nav_applying.get() {
                return;
            }
            let Some(w) = weak.upgrade() else { return };
            let st = nav_current(&app, &w);
            let mut hist = app.nav_hist.borrow_mut();
            let pos = app.nav_pos.get();
            if !hist.is_empty() && hist.get(pos) == Some(&st) {
                return;
            }
            hist.truncate(pos + 1);
            hist.push(st);
            if hist.len() > 100 {
                hist.remove(0);
            }
            app.nav_pos.set(hist.len() - 1);
        }
    });
    let nav_apply: Rc<dyn Fn(&App, &MainWindow, NavState)> = Rc::new(|app: &App, w: &MainWindow, st: NavState| {
        app.nav_applying.set(true);
        w.set_home_level(st.home_level);
        w.set_settings_visible(st.settings);
        w.set_library_tab(st.library);
        match st.detail {
            Some(id) => w.invoke_open_game(id.into()),
            None => *app.detail.borrow_mut() = None,
        }
        ui_refresh();
        app.nav_applying.set(false);
    });
    win.on_nav_back({
        let app = app.clone();
        let weak = win.as_weak();
        let nav_apply = nav_apply.clone();
        move || {
            let Some(w) = weak.upgrade() else { return };
            // On the home carousel, «back» first leaves the console's games.
            if !w.get_settings_visible() && !w.get_detail_visible() && w.get_home_level() == 1 {
                w.set_home_level(0);
                return;
            }
            let pos = app.nav_pos.get();
            if pos == 0 {
                return;
            }
            let st = app.nav_hist.borrow()[pos - 1].clone();
            app.nav_pos.set(pos - 1);
            nav_apply(&app, &w, st);
        }
    });
    win.on_nav_forward({
        let app = app.clone();
        let weak = win.as_weak();
        let nav_apply = nav_apply.clone();
        move || {
            let Some(w) = weak.upgrade() else { return };
            let pos = app.nav_pos.get();
            let next = {
                let hist = app.nav_hist.borrow();
                if pos + 1 >= hist.len() {
                    return;
                }
                hist[pos + 1].clone()
            };
            app.nav_pos.set(pos + 1);
            nav_apply(&app, &w, next);
        }
    });
    // The side buttons of a mouse arrive as raw winit events (Slint's
    // TouchAreas never see them): route them to the history.
    {
        use i_slint_backend_winit::winit::event::{ElementState, MouseButton, WindowEvent};
        use i_slint_backend_winit::{EventResult, WinitWindowAccessor};
        let weak = win.as_weak();
        win.window().on_winit_window_event(move |_, ev| {
            if let WindowEvent::Focused(f) = ev {
                WINDOW_FOCUSED.store(*f, std::sync::atomic::Ordering::Relaxed);
            }
            if let WindowEvent::MouseInput { state: ElementState::Pressed, button, .. } = ev {
                if let Some(w) = weak.upgrade() {
                    match button {
                        MouseButton::Back => {
                            w.invoke_nav_back();
                            return EventResult::PreventDefault;
                        }
                        MouseButton::Forward => {
                            w.invoke_nav_forward();
                            return EventResult::PreventDefault;
                        }
                        _ => {}
                    }
                }
            }
            EventResult::Propagate
        });
    }
    win.invoke_nav_push(); // the starting view is the first history entry

    // Installing is shared by the card/ficha button and the version chooser.
    let start_install: Rc<dyn Fn(String)> = Rc::new({
        let app = app.clone();
        let handle = handle.clone();
        move |id: String| {
            let Some(project) = app.find(&id) else { return };
            app.busy.borrow_mut().insert(id.clone());
            app.install_progress.borrow_mut().insert(id.clone(), 0.0);
            app.install_error.borrow_mut().remove(&id);
            let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            app.cancels.borrow_mut().insert(id.clone(), cancel.clone());
            ui_refresh();
            let client = app.client.clone();
            let paths = app.paths.clone();
            let cfg = store::load_config(&paths).unwrap_or_default();
            let pid = id.clone();
            handle.spawn(async move {
                let prog_id = pid.clone();
                let mut last = -1i32;
                let on_prog = move |done: u64, total: u64| {
                    if total == 0 {
                        return;
                    }
                    let pct = ((done * 100) / total) as i32;
                    if pct == last {
                        return;
                    }
                    last = pct;
                    let idc = prog_id.clone();
                    let f = pct as f32 / 100.0;
                    let _ = slint::invoke_from_event_loop(move || {
                        UI.with(|u| {
                            if let Some((app, _)) = &*u.borrow() {
                                app.install_progress.borrow_mut().insert(idc, f);
                            }
                        });
                        progress_refresh();
                    });
                };
                let err = actions::install_project(&client, &paths, &project, &cfg, Some(cancel), on_prog)
                    .await
                    .err();
                let _ = slint::invoke_from_event_loop(move || {
                    UI.with(|u| {
                        if let Some((app, _)) = &*u.borrow() {
                            app.busy.borrow_mut().remove(&pid);
                            app.install_progress.borrow_mut().remove(&pid);
                            app.size_cache.borrow_mut().remove(&pid);
                            app.cancels.borrow_mut().remove(&pid);
                            // A user cancel is not a failure — no "Reintentar".
                            if err.as_ref().map(|e| !e.is_cancelled()).unwrap_or(false) {
                                app.install_error.borrow_mut().insert(pid.clone());
                            } else {
                                app.install_error.borrow_mut().remove(&pid);
                            }
                        }
                    });
                    ui_refresh();
                });
                refresh_library_size(paths.clone()).await;
            });
        }
    });

    win.on_install({
        let app = app.clone();
        let weak = win.as_weak();
        let start_install = start_install.clone();
        move |id| {
            let id = id.to_string();
            let installed = store::load_installed(&app.paths).unwrap_or_default();
            let show_windows = store::load_config(&app.paths).map(|c| c.show_windows).unwrap_or(false);
            let members = group_members(&app, &id, &installed, show_windows);
            let candidates: Vec<&Project> = members.iter().filter(|m| !installed.contains_key(&m.id)).collect();
            if candidates.len() > 1 && !installed.contains_key(&id) {
                if let Some(w) = weak.upgrade() {
                    let title = candidates[0].original_game.clone();
                    let rows = version_rows(&app, &members, &installed, show_windows, "");
                    let rows: Vec<VersionRow> = rows.into_iter().filter(|r| !r.installed).collect();
                    open_chooser(&app, &w, "install", &format!("Instalar {title}"), rows);
                }
                return;
            }
            start_install(id);
        }
    });

    win.on_cancel_install({
        let app = app.clone();
        move |id| {
            if let Some(flag) = app.cancels.borrow().get(id.as_str()) {
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
    });

    win.on_bulk_dismiss({
        let app = app.clone();
        let weak = win.as_weak();
        move || {
            let Some(w) = weak.upgrade() else { return };
            if w.get_bulk_busy() {
                return;
            }
            app.bulk_items.borrow_mut().clear();
            push_bulk(&app, &w);
            w.set_bulk_label("".into());
        }
    });
    win.on_cancel_update_all({
        let app = app.clone();
        move || {
            // Stop the queue and abort whatever is downloading right now.
            if let Some(flag) = app.bulk_cancel.borrow().as_ref() {
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            for flag in app.cancels.borrow().values() {
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
    });

    // Bulk-update every installed game with a pending update, sequentially,
    // driving both the per-card bars and the sidebar's global progress.
    win.on_update_all({
        let app = app.clone();
        let handle = handle.clone();
        let weak = win.as_weak();
        move || {
            let installed = store::load_installed(&app.paths).unwrap_or_default();
            let targets: Vec<Project> = {
                let catalog = app.catalog.borrow();
                let busy = app.busy.borrow();
                catalog
                    .projects
                    .iter()
                    .filter(|p| installed.get(&p.id).map(|e| has_update(e, &p.cached)).unwrap_or(false))
                    .filter(|p| !busy.contains(&p.id))
                    .cloned()
                    .collect()
            };
            if targets.is_empty() {
                return;
            }
            let total = targets.len();
            *app.bulk_items.borrow_mut() = targets
                .iter()
                .map(|p| {
                    let t = if p.original_game.is_empty() { p.name.clone() } else { p.original_game.clone() };
                    (p.id.clone(), t, 0, 0.0)
                })
                .collect();
            if let Some(w) = weak.upgrade() {
                w.set_bulk_busy(true);
                w.set_bulk_progress(0.0);
                push_bulk(&app, &w);
            }
            // Mark everything queued so the whole batch shows as busy at once.
            for p in &targets {
                app.busy.borrow_mut().insert(p.id.clone());
                app.install_progress.borrow_mut().insert(p.id.clone(), 0.0);
                app.install_error.borrow_mut().remove(&p.id);
            }
            let bulk_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            *app.bulk_cancel.borrow_mut() = Some(bulk_flag.clone());
            ui_refresh();
            let client = app.client.clone();
            let paths = app.paths.clone();
            let cfg = store::load_config(&paths).unwrap_or_default();
            handle.spawn(async move {
                let mut done_ok = 0usize;
                let mut failed = 0usize;
                for (i, p) in targets.iter().enumerate() {
                    if bulk_flag.load(std::sync::atomic::Ordering::Relaxed) {
                        // Cancelled: release everything still queued.
                        let rest: Vec<String> = targets[i..].iter().map(|q| q.id.clone()).collect();
                        let _ = slint::invoke_from_event_loop(move || {
                            UI.with(|u| {
                                if let Some((app, _)) = &*u.borrow() {
                                    for id in &rest {
                                        app.busy.borrow_mut().remove(id);
                                        app.install_progress.borrow_mut().remove(id);
                                    }
                                    for it in app.bulk_items.borrow_mut().iter_mut() {
                                        if rest.contains(&it.0) {
                                            it.2 = 4;
                                        }
                                    }
                                }
                            });
                            ui_refresh();
                        });
                        break;
                    }
                    let pid = p.id.clone();
                    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                    {
                        let (pidc, c) = (pid.clone(), cancel.clone());
                        let _ = slint::invoke_from_event_loop(move || {
                            UI.with(|u| {
                                if let Some((app, weak)) = &*u.borrow() {
                                    if let Some(it) = app.bulk_items.borrow_mut().iter_mut().find(|it| it.0 == pidc) {
                                        it.2 = 1;
                                    }
                                    app.cancels.borrow_mut().insert(pidc, c);
                                    if let Some(w) = weak.upgrade() {
                                        push_bulk(app, &w);
                                    }
                                }
                            });
                        });
                    }
                    let mut last = -1i32;
                    let prog_id = pid.clone();
                    let on_prog = move |done: u64, total_bytes: u64| {
                        if total_bytes == 0 {
                            return;
                        }
                        let pct = ((done * 100) / total_bytes) as i32;
                        if pct == last {
                            return;
                        }
                        last = pct;
                        let f = pct as f32 / 100.0;
                        let overall = (i as f32 + f) / total as f32;
                        let idc = prog_id.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            UI.with(|u| {
                                if let Some((app, weak)) = &*u.borrow() {
                                    if let Some(it) = app.bulk_items.borrow_mut().iter_mut().find(|it| it.0 == idc) {
                                        it.3 = f;
                                    }
                                    app.install_progress.borrow_mut().insert(idc, f);
                                    if let Some(w) = weak.upgrade() {
                                        w.set_bulk_progress(overall);
                                        push_bulk(app, &w);
                                    }
                                }
                            });
                            progress_refresh();
                        });
                    };
                    let err = actions::install_project(&client, &paths, p, &cfg, Some(cancel), on_prog)
                        .await
                        .err();
                    if let Some(e) = &err {
                        eprintln!("[bulk] {} falló: {e}", pid);
                    }
                    match &err {
                        None => done_ok += 1,
                        Some(e) if !e.is_cancelled() => failed += 1,
                        _ => {}
                    }
                    let done = (i + 1) as f32 / total as f32;
                    let _ = slint::invoke_from_event_loop(move || {
                        UI.with(|u| {
                            if let Some((app, weak)) = &*u.borrow() {
                                app.busy.borrow_mut().remove(&pid);
                                app.install_progress.borrow_mut().remove(&pid);
                                app.size_cache.borrow_mut().remove(&pid);
                                app.cancels.borrow_mut().remove(&pid);
                                let failed_now = err.as_ref().map(|e| !e.is_cancelled()).unwrap_or(false);
                                if failed_now {
                                    app.install_error.borrow_mut().insert(pid.clone());
                                }
                                if let Some(it) = app.bulk_items.borrow_mut().iter_mut().find(|it| it.0 == pid) {
                                    it.2 = if failed_now { 3 } else if err.is_some() { 4 } else { 2 };
                                }
                                if let Some(w) = weak.upgrade() {
                                    w.set_bulk_progress(done);
                                    push_bulk(app, &w);
                                }
                            }
                        });
                        ui_refresh();
                    });
                }
                // Summary stays visible under the button when something failed
                // (silent per-card ⚠ alone was easy to miss).
                let summary = if failed > 0 {
                    format!("{done_ok} juegos al día · {failed} con error: reintenta desde su ficha")
                } else if done_ok > 0 {
                    if done_ok == 1 { "1 juego actualizado".to_string() } else { format!("{done_ok} juegos actualizados") }
                } else {
                    String::new()
                };
                let _ = slint::invoke_from_event_loop(move || {
                    UI.with(|u| {
                        if let Some((app, weak)) = &*u.borrow() {
                            *app.bulk_cancel.borrow_mut() = None;
                            if let Some(w) = weak.upgrade() {
                                w.set_bulk_busy(false);
                                push_bulk(app, &w);
                                if !summary.is_empty() {
                                    w.set_notice(summary.into());
                                }
                            }
                        }
                    });
                    ui_refresh();
                });
                refresh_library_size(paths.clone()).await;
            });
        }
    });

    // Returns whether the game process was started; on failure the reason is
    // shown in the notice bar (a silent "failed" flash told the user nothing
    // when the preferred version of a game had no built exe yet).
    let start_play: Rc<dyn Fn(String) -> bool> = Rc::new({
        let app = app.clone();
        let weak = win.as_weak();
        move |id: String| -> bool {
            let Some(project) = app.find(&id) else { return false };
            app.launching.borrow_mut().insert(id.clone(), false); // "Jugando…"
            ui_refresh();
            let mut ok = true;
            if let Err(e) = actions::launch_project(&app.paths, &project) {
                ok = false;
                app.launching.borrow_mut().insert(id.clone(), true); // failed
                if let Some(w) = weak.upgrade() {
                    w.set_notice(format!("No se pudo lanzar {}: {e}", project.name).into());
                }
                ui_refresh();
            }
            // Clear the transient state after a few seconds.
            let idc = id.clone();
            slint::Timer::single_shot(std::time::Duration::from_secs(4), move || {
                UI.with(|u| {
                    if let Some((app, _)) = &*u.borrow() {
                        app.launching.borrow_mut().remove(&idc);
                    }
                });
                ui_refresh();
            });
            ok
        }
    });

    win.on_play({
        let app = app.clone();
        let weak = win.as_weak();
        let start_play = start_play.clone();
        move |id| {
            let id = id.to_string();
            let installed = store::load_installed(&app.paths).unwrap_or_default();
            let cfg = store::load_config(&app.paths).unwrap_or_default();
            let members = group_members(&app, &id, &installed, cfg.show_windows);
            let inst: Vec<&Project> = members.iter().filter(|m| installed.contains_key(&m.id)).collect();
            if inst.len() > 1 {
                let key = members[0].game_key().to_string();
                // A remembered version that can't start (e.g. a builder whose
                // setup hasn't run) falls through to the chooser so the user
                // can pick the one that works; the notice says why.
                if let Some(pref) = cfg.preferred_version.get(&key) {
                    if inst.iter().any(|m| &m.id == pref) && start_play(pref.clone()) {
                        return;
                    }
                }
                if let Some(w) = weak.upgrade() {
                    let title = members[0].original_game.clone();
                    let rows: Vec<VersionRow> = version_rows(&app, &members, &installed, cfg.show_windows, "")
                        .into_iter()
                        .filter(|r| r.installed)
                        .collect();
                    open_chooser(&app, &w, "play", &format!("Jugar a {title}"), rows);
                }
                return;
            }
            start_play(if inst.len() == 1 { inst[0].id.clone() } else { id });
        }
    });

    // Card ✕: several versions installed → ask which; one → the usual confirm.
    win.on_request_uninstall({
        let app = app.clone();
        let weak = win.as_weak();
        move |id| {
            let id = id.to_string();
            let installed = store::load_installed(&app.paths).unwrap_or_default();
            let show_windows = store::load_config(&app.paths).map(|c| c.show_windows).unwrap_or(false);
            let members = group_members(&app, &id, &installed, show_windows);
            let inst: Vec<&Project> = members.iter().filter(|m| installed.contains_key(&m.id)).collect();
            let Some(w) = weak.upgrade() else { return };
            if inst.len() > 1 {
                let title = members[0].original_game.clone();
                let rows: Vec<VersionRow> = version_rows(&app, &members, &installed, show_windows, "")
                    .into_iter()
                    .filter(|r| r.installed)
                    .collect();
                open_chooser(&app, &w, "uninstall", &format!("Eliminar {title}"), rows);
                return;
            }
            let target = inst.first().map(|m| (*m).clone()).or_else(|| app.find(&id));
            if let Some(t) = target {
                open_confirm(&app, &w, &t, &t.original_game);
            }
        }
    });

    win.on_choose_version({
        let app = app.clone();
        let weak = win.as_weak();
        let start_install = start_install.clone();
        let start_play = start_play.clone();
        move |id, remember| {
            let id = id.to_string();
            let mode = app.chooser_mode.borrow().clone();
            match mode.as_str() {
                "romm" => {
                    let rom_id: i64 = id.strip_prefix("romm:").and_then(|x| x.parse().ok()).unwrap_or(-1);
                    let pid = app.romm_project.borrow().clone();
                    let Some(rom) = app.romm_candidates.borrow().iter().find(|r| r.id == rom_id).cloned() else { return };
                    romm_fetch_ui(&app, &pid, rom);
                }
                "install" => start_install(id),
                "play" => {
                    if remember {
                        if let (Some(p), Ok(mut c)) = (app.find(&id), store::load_config(&app.paths)) {
                            c.preferred_version.insert(p.game_key().to_string(), id.clone());
                            let _ = store::save_config(&app.paths, &c);
                        }
                    }
                    start_play(id);
                }
                _ => {
                    if let (Some(p), Some(w)) = (app.find(&id), weak.upgrade()) {
                        open_confirm(&app, &w, &p, &format!("{} ({})", p.original_game, p.name));
                    }
                }
            }
        }
    });

    win.on_set_default_version({
        let app = app.clone();
        move |detail_id, index| {
            let Some(p) = app.find(&detail_id) else { return };
            let installed = store::load_installed(&app.paths).unwrap_or_default();
            let Ok(mut cfg) = store::load_config(&app.paths) else { return };
            let members = group_members(&app, &detail_id, &installed, cfg.show_windows);
            let key = p.game_key().to_string();
            if index <= 0 {
                cfg.preferred_version.remove(&key);
            } else if let Some(m) = members.get(index as usize - 1) {
                cfg.preferred_version.insert(key, m.id.clone());
            }
            let _ = store::save_config(&app.paths, &cfg);
            ui_refresh();
        }
    });

    win.on_confirm_uninstall({
        let app = app.clone();
        let weak = win.as_weak();
        move |id| {
            let (Some(p), Some(w)) = (app.find(&id), weak.upgrade()) else { return };
            let title = if p.original_game.is_empty() { p.name.clone() } else { p.original_game.clone() };
            open_confirm(&app, &w, &p, &title);
        }
    });
    win.on_uninstall({
        let app = app.clone();
        let handle = handle.clone();
        let weak = win.as_weak();
        move |id| {
            let wipe_on = weak.upgrade().map(|w| w.get_confirm_wipe()).unwrap_or(false);
            if let Some(p) = app.find(&id) {
                let wipe = if wipe_on {
                    let installed = store::load_installed(&app.paths).unwrap_or_default();
                    let others: Vec<Project> = installed.keys().filter(|k| **k != p.id).filter_map(|k| app.find(k)).collect();
                    actions::port_data_dirs(&p, &others)
                } else {
                    Vec::new()
                };
                if let Err(e) = actions::uninstall_project_clean(&app.paths, &p, &wipe) {
                    if let Some(w) = weak.upgrade() {
                        w.set_notice(format!("No se pudo eliminar del todo: {e}").into());
                    }
                }
            } else {
                let _ = actions::uninstall_project(&app.paths, &id);
            }
            app.size_cache.borrow_mut().remove(&id.to_string());
            app.install_error.borrow_mut().remove(&id.to_string());
            handle.spawn(refresh_library_size(app.paths.clone()));
            ui_refresh();
        }
    });

    win.on_pick_rom({
        let app = app.clone();
        move |id| {
            let Some(project) = app.find(&id) else { return };
            if let Some(file) = rfd::FileDialog::new()
                .set_title(format!("ROM de {}", project.original_game))
                .pick_file()
            {
                let _ = actions::set_rom(&app.paths, &project, &file.to_string_lossy());
                ui_refresh();
            }
        }
    });

    win.on_import_pkg({
        let app = app.clone();
        move |id| {
            let id = id.to_string();
            let Some(project) = app.find(&id) else { return };
            if app.busy.borrow().contains(&id) {
                return;
            }
            let Some(files) = rfd::FileDialog::new()
                .set_title(format!("PKG de PS4 de {} (juego base y parches)", project.original_game))
                .add_filter("PKG de PS4", &["pkg"])
                .pick_files()
            else {
                return;
            };
            app.busy.borrow_mut().insert(id.clone());
            app.install_progress.borrow_mut().insert(id.clone(), 0.0);
            app.phase.borrow_mut().insert(id.clone(), "Leyendo PKG…".into());
            ui_refresh();
            let paths = app.paths.clone();
            let pid = id.clone();
            std::thread::spawn(move || {
                let prog_id = pid.clone();
                let mut last = (-1i32, String::new());
                let mut on_prog = move |done: u64, total: u64, label: &str| {
                    let pct = (done * 100).checked_div(total).unwrap_or(0) as i32;
                    if pct == last.0 && label == last.1 {
                        return;
                    }
                    last = (pct, label.to_string());
                    let idc = prog_id.clone();
                    let f = pct as f32 / 100.0;
                    let text = format!("Importando {label}");
                    let _ = slint::invoke_from_event_loop(move || {
                        UI.with(|u| {
                            if let Some((app, _)) = &*u.borrow() {
                                app.install_progress.borrow_mut().insert(idc.clone(), f);
                                app.phase.borrow_mut().insert(idc, text);
                            }
                        });
                        progress_refresh();
                    });
                };
                let res = actions::import_ps4_pkgs(&paths, &project, &files, &mut on_prog);
                let _ = slint::invoke_from_event_loop(move || {
                    UI.with(|u| {
                        if let Some((app, weak)) = &*u.borrow() {
                            app.busy.borrow_mut().remove(&pid);
                            app.install_progress.borrow_mut().remove(&pid);
                            app.phase.borrow_mut().remove(&pid);
                            if let Some(w) = weak.upgrade() {
                                match &res {
                                    Ok(dir) => w.set_notice(format!("PKG importado en {}", dir.display()).into()),
                                    Err(e) => w.set_notice(format!("Error al importar el PKG: {e}").into()),
                                }
                            }
                        }
                    });
                    ui_refresh();
                });
            });
        }
    });

    win.on_import_switch({
        let app = app.clone();
        move |id| {
            let id = id.to_string();
            let Some(project) = app.find(&id) else { return };
            if app.busy.borrow().contains(&id) {
                return;
            }
            let Some(weak_win) = UI.with(|u| u.borrow().as_ref().map(|(_, w)| w.clone())) else { return };
            // Installer-style ports open the packages themselves (their own
            // hactool + the user's keys): nstool is only needed to extract.
            if project.rom.setup_cmd.is_none() {
                if let Err(msg) = freeport_core::switch::detect() {
                    if let Some(w) = weak_win.upgrade() {
                        w.set_notice(format!("Para importar volcados de Switch {msg}. Freeport no incluye claves ni herramientas de Nintendo.").into());
                    }
                    return;
                }
            }
            let Some(files) = rfd::FileDialog::new()
                .set_title(format!("Volcado de {} (juego base y actualizaciones)", project.original_game))
                .add_filter("Switch (NSP, XCI, NSZ, XCZ)", &["nsp", "xci", "nsz", "xcz"])
                .pick_files()
            else {
                return;
            };
            app.busy.borrow_mut().insert(id.clone());
            app.install_progress.borrow_mut().insert(id.clone(), 0.0);
            app.phase.borrow_mut().insert(id.clone(), "Preparando…".into());
            ui_refresh();
            let paths = app.paths.clone();
            let pid = id.clone();
            std::thread::spawn(move || {
                let prog_id = pid.clone();
                let mut on_prog = move |label: &str| {
                    let idc = prog_id.clone();
                    let text = label.to_string();
                    let _ = slint::invoke_from_event_loop(move || {
                        UI.with(|u| {
                            if let Some((app, _)) = &*u.borrow() {
                                app.phase.borrow_mut().insert(idc, text);
                            }
                        });
                        ui_refresh();
                    });
                };
                let res = actions::import_switch_dumps(&paths, &project, &files, &mut on_prog);
                let _ = slint::invoke_from_event_loop(move || {
                    UI.with(|u| {
                        if let Some((app, weak)) = &*u.borrow() {
                            app.busy.borrow_mut().remove(&pid);
                            app.install_progress.borrow_mut().remove(&pid);
                            app.phase.borrow_mut().remove(&pid);
                            if let Some(w) = weak.upgrade() {
                                match &res {
                                    Ok(dir) => w.set_notice(format!("Juego importado en {}", dir.display()).into()),
                                    Err(e) => w.set_notice(format!("Error al importar: {e}").into()),
                                }
                            }
                        }
                    });
                    ui_refresh();
                });
            });
        }
    });
    if let Some(ids) = std::env::var("FREEPORT_DEBUG_INSTALL").ok().filter(|s| !s.is_empty()) {
        // dev aid: FREEPORT_DEBUG_INSTALL=<project id>,<id>… installs (or
        // updates) those ports at startup without clicking through the UI.
        for id in ids.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            start_install(id.to_string());
        }
    }
    if let Some(spec) = std::env::var("FREEPORT_DEBUG_IMPORT_SWITCH").ok().filter(|s| !s.is_empty()) {
        // dev aid: FREEPORT_DEBUG_IMPORT_SWITCH=<project id>:<file>,<file>…
        if let Some((id, files)) = spec.split_once(':') {
            if let Some(project) = app.find(id) {
                let files: Vec<std::path::PathBuf> = files.split(',').map(std::path::PathBuf::from).collect();
                let paths = app.paths.clone();
                std::thread::spawn(move || {
                    let r = actions::import_switch_dumps(&paths, &project, &files, &mut |l| eprintln!("[switch] {l}"));
                    eprintln!("[switch] resultado: {r:?}");
                });
            }
        }
    }

    // Land on the library when there is one; the catalog is the storefront,
    // but day-to-day you open the app to play what you already installed.
    if !store::load_installed(&app.paths).unwrap_or_default().is_empty() {
        win.set_library_tab(true);
    }
    if std::env::var_os("FREEPORT_DEBUG_CATALOG").is_some() {
        win.set_library_tab(false); // dev aid: land on the catalog tab
    }
    if let Some(id) = std::env::var("FREEPORT_DEBUG_OPEN").ok().filter(|s| !s.is_empty()) {
        win.invoke_open_game(id.into()); // dev aid: open a game page at startup
    }
    if let Some(spec) = std::env::var("FREEPORT_DEBUG_IMPORT_PKG").ok().filter(|s| !s.is_empty()) {
        // dev aid: FREEPORT_DEBUG_IMPORT_PKG=<project id>:<pkg>,<pkg>… runs the
        // PS4 PKG importer without the file dialog and prints progress.
        if let Some((id, files)) = spec.split_once(':') {
            if let Some(project) = app.find(id) {
                let files: Vec<std::path::PathBuf> = files.split(',').map(std::path::PathBuf::from).collect();
                let paths = app.paths.clone();
                std::thread::spawn(move || {
                    let mut last = -1i32;
                    let r = actions::import_ps4_pkgs(&paths, &project, &files, &mut |d, t, l| {
                        let pct = (d * 100).checked_div(t).unwrap_or(0) as i32;
                        if pct != last {
                            last = pct;
                            eprintln!("[pkg] {l}: {pct}%");
                        }
                    });
                    eprintln!("[pkg] resultado: {r:?}");
                });
            }
        }
    }
    if std::env::var_os("FREEPORT_DEBUG_HOMETEST").is_some() {
        // dev aid: walks the home carousel through every console (one step
        // every 400 ms), to time each move with FREEPORT_DEBUG_TIMING=1.
        let weak = win.as_weak();
        let app2 = app.clone();
        let timer = slint::Timer::default();
        let tick = std::rc::Rc::new(std::cell::Cell::new(0u32));
        // FREEPORT_DEBUG_HOMETEST=games walks the games of the first console instead.
        let games = std::env::var("FREEPORT_DEBUG_HOMETEST").map(|v| v == "games").unwrap_or(false);
        timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(150), move || {
            let Some(w) = weak.upgrade() else { return };
            let t = tick.get();
            tick.set(t + 1);
            if games && w.get_home_level() == 0 {
                home_key(&app2, &w, "a");
                return;
            }
            let n = if games { app2.home_game_ids.borrow().len().min(12) as u32 } else { app2.home_sys_ids.borrow().len() as u32 };
            let fwd = (t / n.max(1)).is_multiple_of(2);
            let k = match (games, fwd) {
                (true, true) => "down",
                (true, false) => "up",
                (false, true) => "right",
                (false, false) => "left",
            };
            let t0 = std::time::Instant::now();
            home_key(&app2, &w, k);
            let _ = t0;
        });
        SCROLLTEST_TIMER.with(|t| *t.borrow_mut() = Some(timer));
    }
    if std::env::var_os("FREEPORT_DEBUG_SCROLLTEST").is_some() {
        // dev aid: synthetic wheel scrolling over the grid, to measure frame
        // rate with SLINT_DEBUG_PERFORMANCE=refresh_lazy,console.
        let weak = win.as_weak();
        let timer = slint::Timer::default();
        let tick = std::rc::Rc::new(std::cell::Cell::new(0u32));
        timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(16), move || {
            let Some(w) = weak.upgrade() else { return };
            let t = tick.get();
            tick.set(t + 1);
            let dir = if (t / 150).is_multiple_of(2) { -1.0 } else { 1.0 };
            w.window().dispatch_event(slint::platform::WindowEvent::PointerScrolled {
                position: slint::LogicalPosition::new(900.0, 500.0),
                delta_x: 0.0,
                delta_y: 40.0 * dir,
            });
        });
        SCROLLTEST_TIMER.with(|t| *t.borrow_mut() = Some(timer));
    }
    if let Ok(sz) = std::env::var("FREEPORT_DEBUG_SIZE") {
        // dev aid: force a window size, e.g. FREEPORT_DEBUG_SIZE=900x700 (after show)
        if let Some((w, h)) = sz.split_once('x') {
            if let (Ok(w), Ok(h)) = (w.parse::<f32>(), h.parse::<f32>()) {
                let weak = win.as_weak();
                slint::Timer::single_shot(std::time::Duration::from_millis(600), move || {
                    if let Some(win) = weak.upgrade() {
                        win.window().set_size(slint::LogicalSize::new(w, h));
                    }
                });
            }
        }
    }
    if let Some(id) = std::env::var("FREEPORT_DEBUG_CHOOSER").ok().filter(|s| !s.is_empty()) {
        // dev aid: open the version chooser (play mode) for a game id
        let installed = store::load_installed(&app.paths).unwrap_or_default();
        let members = group_members(&app, &id, &installed, true);
        let rows = version_rows(&app, &members, &installed, true, "");
        let title = members.first().map(|m| m.original_game.clone()).unwrap_or_default();
        open_chooser(&app, &win, "play", &format!("Jugar a {title}"), rows);
    }
    if let Ok(v) = std::env::var("FREEPORT_DEBUG_SETTINGS") {
        // dev aid: land on the settings screen; "4:n64" = tab 4, console unfolded
        win.set_settings_visible(true);
        let mut it = v.splitn(2, ':');
        if let Some(t) = it.next().and_then(|t| t.parse::<i32>().ok()) {
            win.set_settings_tab(t);
        }
        if let Some(sys) = it.next() {
            win.set_expanded_sys(sys.into());
        }
    }
    if let Ok(v) = std::env::var("FREEPORT_DEBUG_HOME") {
        // dev aid: "explore", "games", "buttons", "explore-games", optional
        // ":<system id>" (after the catalog has loaded)
        let (v, sys) = match v.split_once(':') {
            Some((a, b)) => (a.to_string(), b.to_string()),
            None => (v, String::new()),
        };
        let (app2, weak) = (app.clone(), win.as_weak());
        slint::Timer::single_shot(std::time::Duration::from_millis(1500), move || {
            let Some(w) = weak.upgrade() else { return };
            if v.contains("explore") {
                w.set_library_tab(false);
            }
            if !sys.is_empty() {
                *app2.home_sys_id.borrow_mut() = sys.clone();
            }
            ui_refresh();
            if v.contains("games") || v == "buttons" {
                home_key(&app2, &w, "a");
            }
            if v == "buttons" {
                home_key(&app2, &w, "down");
                home_key(&app2, &w, "right");
            }
        });
    }
    if std::env::var_os("FREEPORT_DEBUG_BULK").is_some() {
        // dev aid: a fake «Actualizar todo» batch, panel open
        *app.bulk_items.borrow_mut() = vec![
            ("a".into(), "Perfect Dark".into(), 2, 1.0),
            ("b".into(), "Super Mario 64".into(), 1, 0.42),
            ("c".into(), "The Wind Waker".into(), 0, 0.0),
        ];
        win.set_bulk_busy(true);
        win.set_bulk_progress(0.45);
        win.set_bulk_open(true);
        push_bulk(&app, &win);
    }
    if std::env::var_os("FREEPORT_DEBUG_UPDATE").is_some() {
        // dev aid: show the self-update banner with fake data (layout checks)
        win.set_update_version("9.9.9".into());
        win.set_update_notes("Notas de la versión de prueba para comprobar el elidido del texto largo en ventanas estrechas".into());
        win.set_update_available(true);
    }

    // NUEVO badge: ids never seen before this session. First run records a
    // baseline instead of flagging the whole catalog.
    {
        let current: Vec<String> = app.catalog.borrow().projects.iter().map(|p| p.id.clone()).collect();
        save_seen(&app.paths, &current);
        // In-memory snapshot AFTER stamping, so this session's additions carry
        // their fresh timestamp (7-day NEW window) instead of being unknown.
        *app.seen.borrow_mut() = load_seen(&app.paths);
    }

    win.on_add_steam({
        let app = app.clone();
        move |id| {
            let Some(p) = app.find(&id) else { return };
            let title = if p.original_game.is_empty() { p.name.clone() } else { p.original_game.clone() };
            let cover = p
                .box_art
                .as_ref()
                .or(p.cover_url.as_ref())
                .map(|u| thumbs::path_for(&app.paths, u))
                .filter(|f| f.exists());
            let msg = match freeport_core::steam::add_shortcut(&p.id, &title, cover.as_deref()) {
                Ok(_) => "Añadido a Steam ✓ — reinicia Steam para verlo".to_string(),
                Err(e) => format!("Steam: {e}"),
            };
            if let Some(st) = app.detail.borrow_mut().as_mut() {
                st.msg = msg;
            }
            ui_refresh();
        }
    });

    handle.spawn(refresh_library_size(app.paths.clone()));

    rebuild(&app, &win);
    spawn_missing_thumbs(&app, &handle);

    // Refresh the catalog from the remote repo on startup, then rebuild.
    {
        let client = app.client.clone();
        let paths = app.paths.clone();
        let url = store::load_config(&paths).ok().and_then(|c| c.catalog_url);
        let handle2 = handle.clone();
        handle.spawn(async move {
            match actions::refresh_catalog(&client, &paths, url.as_deref()).await {
                Ok(cat) => {
                    let _ = slint::invoke_from_event_loop(move || {
                        UI.with(|u| {
                            if let Some((app, _)) = &*u.borrow() {
                                *app.catalog.borrow_mut() = cat;
                            }
                        });
                        ui_refresh();
                        UI.with(|u| {
                            if let Some((app, _)) = &*u.borrow() {
                                spawn_missing_thumbs(app, &handle2);
                            }
                        });
                    });
                }
                Err(e) => {
                    // Never fail silently: a stale local copy looks like lost
                    // games and covers to the user.
                    eprintln!("[freeport] catálogo remoto no disponible: {e}");
                    let msg = format!("No se pudo cargar el catálogo remoto ({e}); se muestra la copia local.");
                    let _ = slint::invoke_from_event_loop(move || {
                        UI.with(|u| {
                            if let Some((_, weak)) = &*u.borrow() {
                                if let Some(w) = weak.upgrade() {
                                    w.set_notice(msg.into());
                                }
                            }
                        });
                    });
                }
            }
        });
    }

    win.on_do_update({
        let app = app.clone();
        let handle = handle.clone();
        let weak = win.as_weak();
        move || {
            let Some(upd) = app.pending_update.borrow().clone() else { return };
            if let Some(w) = weak.upgrade() {
                w.set_update_busy(true);
                w.set_update_error(false);
            }
            let client = app.client.clone();
            handle.spawn(async move {
                if let Err(e) = update::apply(&client, &upd).await {
                    eprintln!("[freeport] auto-update: {e}");
                    let _ = slint::invoke_from_event_loop(move || {
                        UI.with(|u| {
                            if let Some((_, weak)) = &*u.borrow() {
                                if let Some(w) = weak.upgrade() {
                                    w.set_update_busy(false);
                                    w.set_update_available(true); // keep banner to retry
                                    w.set_update_error(true);
                                    w.set_update_notes(e.into()); // tell the user WHY
                                }
                            }
                        });
                    });
                }
                // On success apply() replaces the process and never returns.
            });
        }
    });

    win.on_dismiss_update({
        let app = app.clone();
        let weak = win.as_weak();
        move || {
            if let Some(w) = weak.upgrade() {
                let ver = w.get_update_version().to_string();
                if let Ok(mut cfg) = store::load_config(&app.paths) {
                    cfg.skip_version = Some(ver);
                    let _ = store::save_config(&app.paths, &cfg);
                }
                w.set_update_available(false);
            }
        }
    });

    // Manual "Buscar actualizaciones" (from Settings).
    win.on_check_updates({
        let app = app.clone();
        let handle = handle.clone();
        let weak = win.as_weak();
        move || {
            if let Some(w) = weak.upgrade() {
                w.set_settings_msg("Comprobando…".into());
            }
            handle.spawn(run_update_check(app.client.clone(), true));
        }
    });

    // Check for app updates on startup, then every 6 hours.
    handle.spawn(run_update_check(app.client.clone(), false));
    {
        let client = app.client.clone();
        let handle2 = handle.clone();
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_secs(6 * 3600),
            move || {
                handle2.spawn(run_update_check(client.clone(), false));
            },
        );
        UPDATE_TIMER.with(|t| *t.borrow_mut() = Some(timer));
    }

    // Refresh the catalog every 6 hours while the app stays open (new games
    // and probe-updated tags land without a restart).
    {
        let weak = win.as_weak();
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_secs(6 * 3600),
            move || {
                if let Some(w) = weak.upgrade() {
                    if !w.get_catalog_busy() {
                        w.invoke_refresh_catalog();
                    }
                }
            },
        );
        CATALOG_TIMER.with(|t| *t.borrow_mut() = Some(timer));
    }

    // Clock in the home's top bar (local time, refreshed every 15 s).
    {
        fn now_hm() -> String {
            chrono::Local::now().format("%H:%M").to_string()
        }
        win.set_clock(now_hm().into());
        let weak = win.as_weak();
        let timer = slint::Timer::default();
        timer.start(slint::TimerMode::Repeated, std::time::Duration::from_secs(15), move || {
            if let Some(w) = weak.upgrade() {
                w.set_clock(now_hm().into());
            }
        });
        CLOCK_TIMER.with(|t| *t.borrow_mut() = Some(timer));
    }

    // `--tv`: boot straight into the console view, full screen (Sunshine
    // launches it like this for streaming to the TV).
    if std::env::args().any(|a| a == "--tv") {
        win.set_fullscreen(true);
        win.window().set_fullscreen(true);
    }

    // Dev aid: open a game page straight away (screenshot/UI iteration).
    if let Ok(id) = std::env::var("FREEPORT_DEBUG_DETAIL") {
        win.invoke_open_game(id.into());
    }
    if std::env::var_os("FREEPORT_DEBUG_UPDATE_ALL").is_some() {
        win.invoke_update_all();
    }

    println!("[freeport] plataforma {}", app.triple);
    win.run()?;
    // Dropping the tokio Runtime waits for every spawn_blocking task (e.g.
    // extracting a multi-GB archive) and lingering downloads, which left the
    // process hanging after the window closed. Give in-flight work a short
    // grace period, then exit unconditionally — installs are crash-safe
    // (.part files + reinstall wipes the app dir).
    rt.shutdown_timeout(std::time::Duration::from_secs(2));
    update::hard_exit(0);
}
