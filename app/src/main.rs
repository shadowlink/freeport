// Freeport — native UI (Slint) on top of freeport-core (reused backend).
// Phase 1-2: catalog view + install/launch/rom/uninstall + on-demand thumbnails.

slint::include_modules!();

#[cfg(windows)]
mod win_titlebar;

use freeport_core::model::{Catalog, Project};
use freeport_core::store::{self, Paths};
use freeport_core::mods::ModInfo;
use freeport_core::wiki::WikiInfo;
use freeport_core::{actions, gamepad, platform, ra, thumbs, update, wiki};
use i_slint_backend_winit::WinitWindowAccessor;
use slint::{Color, Image, ModelRc, SharedString, VecModel, Weak};
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
    ("amiga", include_bytes!("../assets/logos/amiga.svg")),
    ("gb", include_bytes!("../assets/logos/gb.svg")),
    ("gba", include_bytes!("../assets/logos/gba.svg")),
    ("gc", include_bytes!("../assets/logos/gc.svg")),
    ("genesis", include_bytes!("../assets/logos/genesis.svg")),
    ("n64", include_bytes!("../assets/logos/n64.svg")),
    ("nds", include_bytes!("../assets/logos/nds.svg")),
    ("pc", include_bytes!("../assets/logos/pc.svg")),
    ("psx", include_bytes!("../assets/logos/psx.svg")),
    ("segacd", include_bytes!("../assets/logos/segacd.svg")),
    ("snes", include_bytes!("../assets/logos/snes.svg")),
    ("wii", include_bytes!("../assets/logos/wii.svg")),
    ("x360", include_bytes!("../assets/logos/x360.svg")),
];

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
    detail: RefCell<Option<DetailState>>,
    mods_cache: RefCell<std::collections::HashMap<String, Vec<ModInfo>>>,
    mod_busy: RefCell<HashSet<String>>,
    mod_progress: RefCell<std::collections::HashMap<String, (f32, String)>>,
    mod_icons: RefCell<std::collections::HashMap<String, Image>>,
    screens_cache: RefCell<std::collections::HashMap<String, Vec<String>>>,
    cover_cache: RefCell<std::collections::HashMap<String, Image>>,
    hero_cache: RefCell<std::collections::HashMap<String, Image>>,
    size_cache: RefCell<std::collections::HashMap<String, u64>>,
    // id -> (tag, published_at ISO, changelog body)
    changelog_cache: RefCell<std::collections::HashMap<String, (String, String, String)>>,
    tv_shelves: RefCell<Vec<(String, Vec<String>)>>,
    pending_update: RefCell<Option<update::Update>>,
    /// Transient per-game launch state shown inline on the Play button:
    /// present+false = "Jugando…", present+true = launch failed.
    launching: RefCell<std::collections::HashMap<String, bool>>,
    /// Games whose last install attempt failed (shows "Reintentar" inline).
    install_error: RefCell<HashSet<String>>,
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

thread_local! {
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
    match &p.cached {
        Some(c) if !c.platforms.is_empty() => c.platforms.iter().any(|x| x == triple),
        _ => p.asset_rules.contains_key(triple),
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
        (native || win_ok || inst.is_some(), !native && (win_ok || inst.map(|e| e.windows).unwrap_or(false)))
    }

    fn find(&self, id: &str) -> Option<Project> {
        self.catalog.borrow().projects.iter().find(|p| p.id == id).cloned()
    }
}

fn load_logos(dir: &Path) -> std::collections::HashMap<String, Image> {
    let _ = std::fs::create_dir_all(dir);
    let mut map = std::collections::HashMap::new();
    for (id, bytes) in LOGOS {
        let f = dir.join(format!("{id}.svg"));
        if !f.exists() {
            let _ = std::fs::write(&f, bytes);
        }
        if let Ok(img) = Image::load_from_path(&f) {
            map.insert((*id).to_string(), img);
        }
    }
    map
}

fn rebuild(app: &App, win: &MainWindow) {
    let installed = store::load_installed(&app.paths).unwrap_or_default();
    let active = win.get_active_system().to_string();
    let library = win.get_library_tab();
    let query = win.get_search_text().to_lowercase();
    let sort_mode = win.get_sort_mode().to_string();
    let filter_kind = win.get_filter_kind().to_string();
    let filter_plat = win.get_filter_plat().to_string();
    let filter_genre = win.get_filter_genre().to_string();
    let seen = app.seen.borrow();
    let now_secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let busy = app.busy.borrow();
    let catalog = app.catalog.borrow();
    let cfg = store::load_config(&app.paths).unwrap_or_default();
    let show_windows = cfg.show_windows;
    let favs: std::collections::HashSet<&str> = cfg.favorites.iter().map(|s| s.as_str()).collect();

    let mut sys_rows: Vec<SysRow> = Vec::new();
    for s in &catalog.systems {
        let count = catalog
            .projects
            .iter()
            .filter(|p| p.system == s.id && app.visibility(p, &installed, show_windows).0)
            .filter(|p| !library || installed.contains_key(&p.id))
            .count();
        if count == 0 {
            continue;
        }
        let logo = app.logos.get(&s.id).cloned();
        sys_rows.push(SysRow {
            id: s.id.clone().into(),
            name: s.name.clone().into(),
            count: count as i32,
            color: parse_color(&s.color),
            logo: logo.clone().unwrap_or_default(),
            has_logo: logo.is_some(),
        });
    }

    // (favorite, name_lc, year, last_played_epoch, card) for sorting.
    let mut sortable: Vec<(bool, String, i64, i64, CardItem)> = Vec::new();
    for p in &catalog.projects {
        let (visible, is_win) = app.visibility(p, &installed, show_windows);
        if !visible
            || (library && !installed.contains_key(&p.id))
            || (!active.is_empty() && p.system != active)
        {
            continue;
        }
        if !filter_kind.is_empty() && p.kind != filter_kind {
            continue;
        }
        if (filter_plat == "native" && is_win) || (filter_plat == "windows" && !is_win) {
            continue;
        }
        if !filter_genre.is_empty() && p.genre.as_deref() != Some(filter_genre.as_str()) {
            continue;
        }
        if !query.is_empty() {
            let sys_name = catalog.systems.iter().find(|s| s.id == p.system).map(|s| s.name.as_str()).unwrap_or("");
            let hay = format!(
                "{} {} {} {}",
                p.original_game, p.name, sys_name, p.genre.clone().unwrap_or_default()
            )
            .to_lowercase();
            if !hay.contains(&query) {
                continue;
            }
        }
        let entry = installed.get(&p.id);
        let update = entry.map(|e| has_update(e, &p.cached)).unwrap_or(false);
        let sys_color = catalog
            .systems
            .iter()
            .find(|s| s.id == p.system)
            .map(|s| parse_color(&s.color))
            .unwrap_or(Color::from_rgb_u8(0x88, 0x88, 0x88));
        let title = if p.original_game.is_empty() { p.name.clone() } else { p.original_game.clone() };
        let is_fav = favs.contains(p.id.as_str());
        let year = p.year.unwrap_or(0) as i64;
        let last = entry.and_then(|e| e.last_played.as_ref()).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
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
            is_fav,
            name_lc,
            year,
            last,
            CardItem {
                id: p.id.clone().into(),
                title: title.into(),
                subtitle: p.name.clone().into(),
                cover: app.cover(p),
                installed: entry.is_some(),
                is_windows: is_win,
                update_available: update,
                needs_rom: p.rom.mode == "copy",
                rom_ok: entry.and_then(|e| e.rom_path.as_ref()).is_some(),
                busy: busy.contains(&p.id),
                progress: app.install_progress.borrow().get(&p.id).copied().unwrap_or(0.0),
                kind: if p.kind == "recompilation" { "RECOMP" } else { "PORT" }.into(),
                sys_color,
                favorite: is_fav,
                play_state,
                install_error: install_err,
                version: version.into(),
                new_version: if update { latest_tag.into() } else { "".into() },
                meta: if last > 0 { format!("Jugado {}", fmt_ago(last)).into() } else { "".into() },
                is_new: is_new_id(&seen, &p.id, now_secs),
            },
        ));
    }
    // Favorites first, then by the selected sort mode.
    sortable.sort_by(|a, b| {
        b.0.cmp(&a.0).then_with(|| match sort_mode.as_str() {
            "year" => b.2.cmp(&a.2),
            "recent" => b.3.cmp(&a.3),
            _ => a.1.cmp(&b.1),
        })
    });
    // "Seguir jugando": most recently played installed games (library shelf).
    let mut recent_pool: Vec<(i64, CardItem)> = sortable
        .iter()
        .filter(|t| t.3 > 0 && t.4.installed)
        .map(|t| (t.3, t.4.clone()))
        .collect();
    recent_pool.sort_by(|a, b| b.0.cmp(&a.0));
    let recent: Vec<CardItem> = recent_pool.into_iter().take(4).map(|t| t.1).collect();

    let cards: Vec<CardItem> = sortable.into_iter().map(|t| t.4).collect();

    let count = cards.len() as i32;
    let cols = (win.get_cols().max(1)) as usize;
    let rows: Vec<ModelRc<CardItem>> = cards
        .chunks(cols)
        .map(|c| ModelRc::new(VecModel::from(c.to_vec())))
        .collect();

    let header = if !active.is_empty() {
        catalog.systems.iter().find(|s| s.id == active).map(|s| s.name.clone()).unwrap_or(active)
    } else if library {
        "Mi biblioteca".to_string()
    } else {
        "Catálogo".to_string()
    };

    // Pending updates across ALL installed games (ignores tab/filter/search),
    // so the sidebar badge is always truthful.
    let pending = catalog
        .projects
        .iter()
        .filter(|p| installed.get(&p.id).map(|e| has_update(e, &p.cached)).unwrap_or(false))
        .count();

    // "Nuevas incorporaciones": catalog additions from the last week, newest
    // first (appended at the end of the catalog file), for the showcase strip.
    let mut newcomers: Vec<CardItem> = Vec::new();
    for p in catalog.projects.iter().rev() {
        if newcomers.len() >= 8 {
            break;
        }
        let (visible, is_win) = app.visibility(p, &installed, show_windows);
        if !visible || !is_new_id(&seen, &p.id, now_secs) {
            continue;
        }
        let sys_name = catalog.systems.iter().find(|s| s.id == p.system).map(|s| s.name.clone()).unwrap_or_default();
        let meta = match p.year {
            Some(y) => format!("{sys_name} · {y}"),
            None => sys_name,
        };
        newcomers.push(CardItem {
            id: p.id.clone().into(),
            title: (if p.original_game.is_empty() { p.name.clone() } else { p.original_game.clone() }).into(),
            subtitle: p.name.clone().into(),
            cover: app.cover(p),
            installed: installed.contains_key(&p.id),
            is_windows: is_win,
            update_available: false,
            needs_rom: false,
            rom_ok: false,
            busy: false,
            progress: 0.0,
            kind: if p.kind == "recompilation" { "RECOMP" } else { "PORT" }.into(),
            sys_color: Color::from_rgb_u8(0x88, 0x88, 0x88),
            favorite: false,
            play_state: 0,
            install_error: false,
            version: "".into(),
            new_version: "".into(),
            meta: meta.into(),
            is_new: true,
        });
    }

    // Genre filter options: every distinct curated genre.
    let mut genres: Vec<String> = catalog.projects.iter().filter_map(|p| p.genre.clone()).collect();
    genres.sort();
    genres.dedup();
    let mut genre_labels: Vec<SharedString> = vec!["Todos los géneros".into()];
    genre_labels.extend(genres.into_iter().map(SharedString::from));

    win.set_systems(ModelRc::new(VecModel::from(sys_rows)));
    win.set_rows(ModelRc::new(VecModel::from(rows)));
    win.set_header_title(header.into());
    win.set_header_count(count);
    win.set_updates_pending(pending as i32);
    win.set_recent(ModelRc::new(VecModel::from(recent)));
    win.set_newcomers(ModelRc::new(VecModel::from(newcomers)));
    win.set_genre_labels(ModelRc::new(VecModel::from(genre_labels)));
}

impl App {
    fn cover(&self, p: &Project) -> Image {
        let Some(url) = p.box_art.as_ref().or(p.cover_url.as_ref()) else { return Image::default() };
        if let Some(img) = self.cover_cache.borrow().get(url) {
            return img.clone();
        }
        let path = thumbs::path_for(&self.paths, url);
        if !path.exists() {
            return Image::default();
        }
        match Image::load_from_path(&path) {
            Ok(img) => {
                self.cover_cache.borrow_mut().insert(url.clone(), img.clone());
                img
            }
            Err(_) => Image::default(),
        }
    }
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
        "Tipo",
        if p.kind == "recompilation" { "Recompilación estática" } else { "Port nativo" }.into(),
        if p.kind == "recompilation" { parse_color("#ffb23e") } else { parse_color("#3fa7e0") },
    );
    if let Some(y) = p.year {
        fact("Año", format!("{y}"), white);
    }
    fact("Desarrollador", p.developer.clone().unwrap_or_default(), white);
    fact("Género", p.genre.clone().unwrap_or_default(), white);
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
        fact("Se ejecuta con", "Wine / Proton".into(), parse_color("#3fa7e0"));
    }

    let related: Vec<CardItem> = catalog
        .projects
        .iter()
        .filter(|r| r.id != p.id && r.system == p.system && app.visibility(r, &installed, show_windows).0)
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
        })
        .collect();

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
    let data = DetailData {
        id: p.id.clone().into(),
        title: title.into(),
        subtitle: p.name.clone().into(),
        cover: app.cover(&p),
        installed: entry.is_some(),
        is_windows: is_win,
        update_available: update,
        needs_rom: p.rom.mode == "copy",
        rom_ok: entry.and_then(|e| e.rom_path.as_ref()).is_some(),
        busy: app.busy.borrow().contains(&p.id),
        progress: app.install_progress.borrow().get(&p.id).copied().unwrap_or(0.0),
        play_state: match app.launching.borrow().get(&p.id) { Some(false) => 1, Some(true) => 2, None => 0 },
        install_error: app.install_error.borrow().contains(&p.id),
        stats: stats.into(),
        repo_url: format!("https://github.com/{}", p.repo.slug()).into(),
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
        has_mods: !mod_rows.is_empty(),
        has_screens: !screens.is_empty(),
        ra_eligible: p.ra_supported && freeport_core::ra_mod::platform_supported(),
        ra_enabled: p.ra_supported && freeport_core::ra_mod::is_enabled(&p),
        ra_beta: p.ra_beta,
        kind: if p.kind == "recompilation" { "RECOMP" } else { "PORT" }.into(),
        version: installed_tag.into(),
        new_version: if update { latest_tag.into() } else { "".into() },
        facts: ModelRc::new(VecModel::from(facts)),
        ra_summary: ra_summary.into(),
        badge_rows: ModelRc::new(VecModel::from(badge_rows)),
        related: ModelRc::new(VecModel::from(related)),
        mods: ModelRc::new(VecModel::from(mod_rows)),
        screens: ModelRc::new(VecModel::from(screens)),
    };
    win.set_detail_msg(state.msg.clone().into());
    win.set_detail(data);
    win.set_detail_visible(true);
}

/// Rebuild the UI from the current app state (runs on the UI thread).
fn ui_refresh() {
    UI.with(|u| {
        if let Some((app, weak)) = &*u.borrow() {
            if let Some(w) = weak.upgrade() {
                rebuild(app, &w);
                build_detail(app, &w);
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

/// Releases Slint's pointer grab after a window drag/resize hands the pointer to
/// the compositor. Without this the TouchArea keeps the grab (it never receives a
/// `Released`, only a `CursorLeft`→`Exit`), which freezes all further UI input.
/// Deferred to the next loop iteration to avoid re-entrant input dispatch.
fn release_pointer_grab(weak: &Weak<MainWindow>) {
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(w) = weak.upgrade() {
            w.window().dispatch_event(slint::platform::WindowEvent::PointerReleased {
                position: slint::LogicalPosition::new(0.0, 0.0),
                button: slint::platform::PointerEventButton::Left,
            });
        }
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

// ── TV / Big Picture ──────────────────────────────────────────────────

fn tv_hero(app: &App, win: &MainWindow) {
    let shelves = app.tv_shelves.borrow();
    let s = win.get_tv_shelf().max(0) as usize;
    let c = win.get_tv_col().max(0) as usize;
    if let Some((_, ids)) = shelves.get(s) {
        if let Some(id) = ids.get(c) {
            if let Some(p) = app.find(id) {
                let title = if p.original_game.is_empty() { p.name.clone() } else { p.original_game.clone() };
                // Prefer the full-res hero art (prefetched); fall back to the thumb.
                let art = p.box_art.as_ref().or(p.cover_url.as_ref());
                let img = art
                    .and_then(|u| app.hero_cache.borrow().get(u).cloned())
                    .unwrap_or_else(|| app.cover(&p));
                win.set_tv_hero_cover(img);
                win.set_tv_hero_title(title.into());
                win.set_tv_hero_subtitle(p.name.clone().into());
                return;
            }
        }
    }
    win.set_tv_hero_title("".into());
    win.set_tv_hero_subtitle("".into());
}

fn build_tv(app: &App, win: &MainWindow) {
    let installed = store::load_installed(&app.paths).unwrap_or_default();
    let catalog = app.catalog.borrow();
    let mut bounds: Vec<(String, Vec<String>)> = Vec::new();
    let mut shelves: Vec<TvShelf> = Vec::new();
    for s in &catalog.systems {
        // TV / Big Picture is a couch experience: show only installed games.
        let games: Vec<&Project> = catalog
            .projects
            .iter()
            .filter(|p| p.system == s.id && installed.contains_key(&p.id))
            .collect();
        if games.is_empty() {
            continue;
        }
        let sys_color = parse_color(&s.color);
        let cards: Vec<CardItem> = games
            .iter()
            .map(|p| {
                let entry = installed.get(&p.id);
                let update = entry.map(|e| has_update(e, &p.cached)).unwrap_or(false);
                let latest = p.cached.as_ref().and_then(|c| c.latest_tag.clone()).unwrap_or_default();
                CardItem {
                    id: p.id.clone().into(),
                    title: (if p.original_game.is_empty() { p.name.clone() } else { p.original_game.clone() }).into(),
                    subtitle: p.name.clone().into(),
                    cover: app.cover(p),
                    installed: entry.is_some(),
                    is_windows: false,
                    update_available: update,
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
                    new_version: if update { latest.into() } else { "".into() },
                    meta: "".into(),
                    is_new: false,
                }
            })
            .collect();
        bounds.push((s.name.clone(), games.iter().map(|p| p.id.clone()).collect()));
        shelves.push(TvShelf {
            name: s.name.clone().into(),
            count: cards.len() as i32,
            games: ModelRc::new(VecModel::from(cards)),
        });
    }
    let n = shelves.len() as i32;
    *app.tv_shelves.borrow_mut() = bounds;
    win.set_tv_shelves(ModelRc::new(VecModel::from(shelves)));
    win.set_tv_shelf_count(n);
    win.set_tv_shelf(0);
    win.set_tv_col(0);
    tv_hero(app, win);
}

fn exit_tv(win: &MainWindow) {
    win.set_tv_visible(false);
    let _ = win.window().set_fullscreen(false);
}

fn tv_input(app: &App, win: &MainWindow, button: &str) {
    let shelves = app.tv_shelves.borrow();
    if shelves.is_empty() {
        return;
    }
    let count = shelves.len() as i32;
    let len_of = |i: i32| shelves.get(i.max(0) as usize).map(|(_, ids)| ids.len() as i32).unwrap_or(0);
    let mut s = win.get_tv_shelf();
    let mut c = win.get_tv_col();
    match button {
        "down" => {
            if s + 1 < count {
                s += 1;
                c = 0; // start each new row at its first item
            }
        }
        "up" => {
            if s > 0 {
                s -= 1;
                c = 0;
            }
        }
        "right" => {
            if c + 1 < len_of(s) {
                c += 1;
            }
        }
        "left" => {
            if c > 0 {
                c -= 1;
            }
        }
        "rb" => {
            if s + 1 < count {
                s += 1;
                c = 0;
            }
        }
        "lb" => {
            if s > 0 {
                s -= 1;
                c = 0;
            }
        }
        "a" => {
            if let Some((_, ids)) = shelves.get(s as usize) {
                if let Some(id) = ids.get(c as usize) {
                    if let Some(p) = app.find(id) {
                        // TV only lists installed games, so A always launches.
                        let _ = actions::launch_project(&app.paths, &p);
                    }
                }
            }
            return;
        }
        "y" => {
            // Update the focused game if it has one pending.
            if let Some(id) = shelves.get(s as usize).and_then(|(_, ids)| ids.get(c as usize)).cloned() {
                drop(shelves);
                let installed = store::load_installed(&app.paths).unwrap_or_default();
                if let Some(p) = app.find(&id) {
                    let has = installed.get(&id).map(|e| has_update(e, &p.cached)).unwrap_or(false);
                    if has && !app.busy.borrow().contains(&id) {
                        win.invoke_install(id.into());
                    }
                }
            }
            return;
        }
        "b" | "start" | "back" => {
            drop(shelves);
            exit_tv(win);
            return;
        }
        _ => {}
    }
    win.set_tv_shelf(s);
    win.set_tv_col(c);
    drop(shelves);
    tv_hero(app, win);
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
    let total = tokio::task::spawn_blocking(move || {
        let installed = store::load_installed(&paths).unwrap_or_default();
        installed.values().map(|e| dir_size(Path::new(&e.install_path))).sum::<u64>()
    })
    .await
    .unwrap_or(0);
    let _ = slint::invoke_from_event_loop(move || {
        UI.with(|u| {
            if let Some((_, weak)) = &*u.borrow() {
                if let Some(w) = weak.upgrade() {
                    w.set_library_size(if total > 0 { fmt_bytes(total).into() } else { "".into() });
                }
            }
        });
    });
}

fn apply_card_size(win: &MainWindow, size: &str) {
    let w = match size {
        "s" => 126.0,
        "l" => 196.0,
        _ => 158.0,
    };
    win.set_card_w(w);
    win.set_card_size(if size == "s" || size == "l" { size.into() } else { "m".into() });
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--play") {
        let id = args.get(i + 1).cloned().unwrap_or_default();
        return cli_play(&id);
    }
    // Force the winit backend so we can reach the underlying window (drag/resize/
    // minimize/maximize) via WinitWindowAccessor. Decorations are turned off with
    // the Slint `no-frame` property (the backend would otherwise re-enable them).
    let backend = i_slint_backend_winit::Backend::new().expect("winit backend");
    slint::platform::set_platform(Box::new(backend)).expect("set winit platform");

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
        detail: RefCell::new(None),
        mods_cache: RefCell::new(std::collections::HashMap::new()),
        mod_busy: RefCell::new(HashSet::new()),
        mod_progress: RefCell::new(std::collections::HashMap::new()),
        mod_icons: RefCell::new(std::collections::HashMap::new()),
        screens_cache: RefCell::new(std::collections::HashMap::new()),
        cover_cache: RefCell::new(std::collections::HashMap::new()),
        hero_cache: RefCell::new(std::collections::HashMap::new()),
        size_cache: RefCell::new(std::collections::HashMap::new()),
        changelog_cache: RefCell::new(std::collections::HashMap::new()),
        tv_shelves: RefCell::new(Vec::new()),
        pending_update: RefCell::new(None),
        launching: RefCell::new(std::collections::HashMap::new()),
        install_error: RefCell::new(HashSet::new()),
        seen: RefCell::new(std::collections::HashMap::new()),
        ra_cache: RefCell::new(std::collections::HashMap::new()),
        ra_badges: RefCell::new(std::collections::HashMap::new()),
        cancels: RefCell::new(std::collections::HashMap::new()),
        bulk_cancel: RefCell::new(None),
    });

    let win = MainWindow::new()?;
    UI.with(|u| *u.borrow_mut() = Some((app.clone(), win.as_weak())));

    // Controller input → route to TV navigation when Big Picture is open.
    gamepad::start(|b| {
        let b = b.to_string();
        let _ = slint::invoke_from_event_loop(move || {
            UI.with(|u| {
                if let Some((app, weak)) = &*u.borrow() {
                    if let Some(w) = weak.upgrade() {
                        if w.get_tv_visible() {
                            tv_input(app, &w, &b);
                        }
                    }
                }
            });
        });
    });

    win.set_sort_mode("name".into());

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
        win.set_runner_labels(ModelRc::new(VecModel::from(labels)));
        let current = cfg
            .wine_runner
            .as_ref()
            .and_then(|id| app.runners.iter().find(|r| &r.id == id))
            .map(|r| r.label.clone())
            .unwrap_or_else(|| "Automático".to_string());
        win.set_current_runner_label(current.into());
    }

    // ── Custom titlebar / frameless window controls ──────────────────────
    win.on_win_minimize({
        let weak = win.as_weak();
        move || {
            if let Some(w) = weak.upgrade() {
                w.window().with_winit_window(|ww| ww.set_minimized(true));
            }
        }
    });
    win.on_win_maximize({
        let weak = win.as_weak();
        move || {
            if let Some(w) = weak.upgrade() {
                let now = w
                    .window()
                    .with_winit_window(|ww| {
                        let next = !ww.is_maximized();
                        ww.set_maximized(next);
                        next
                    })
                    .unwrap_or(false);
                w.set_win_maximized(now);
            }
        }
    });
    win.on_win_close(|| {
        let _ = slint::quit_event_loop();
    });
    win.on_title_press({
        let weak = win.as_weak();
        move || {
            if let Some(w) = weak.upgrade() {
                w.window().with_winit_window(|ww| {
                    let _ = ww.drag_window();
                });
                release_pointer_grab(&weak);
            }
        }
    });
    win.on_resize_press({
        let weak = win.as_weak();
        move |dir| {
            use i_slint_backend_winit::winit::window::ResizeDirection as RD;
            let d = match dir.as_str() {
                "n" => RD::North,
                "s" => RD::South,
                "e" => RD::East,
                "w" => RD::West,
                "ne" => RD::NorthEast,
                "nw" => RD::NorthWest,
                "se" => RD::SouthEast,
                "sw" => RD::SouthWest,
                _ => return,
            };
            if let Some(w) = weak.upgrade() {
                w.window().with_winit_window(|ww| {
                    let _ = ww.drag_resize_window(d);
                });
                release_pointer_grab(&weak);
            }
        }
    });

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

    win.on_open_tv({
        let app = app.clone();
        let handle = handle.clone();
        let weak = win.as_weak();
        move || {
            if let Some(w) = weak.upgrade() {
                build_tv(&app, &w);
                w.set_tv_visible(true);
                let _ = w.window().set_fullscreen(true);
            }
            // Prefetch full-resolution hero art for installed games (the grid
            // thumbnails are only 320px and look pixelated blown up in the hero).
            let installed = store::load_installed(&app.paths).unwrap_or_default();
            let urls: Vec<String> = app
                .catalog
                .borrow()
                .projects
                .iter()
                .filter(|p| installed.contains_key(&p.id))
                .filter_map(|p| p.box_art.clone().or_else(|| p.cover_url.clone()))
                .filter(|u| !app.hero_cache.borrow().contains_key(u))
                .collect();
            if urls.is_empty() {
                return;
            }
            let client = app.client.clone();
            let paths = app.paths.clone();
            handle.spawn(async move {
                let mut fetched: Vec<(String, String)> = Vec::new();
                for u in urls {
                    if let Ok(p) = thumbs::get_full(&client, &paths, &u).await {
                        fetched.push((u, p.display().to_string()));
                    }
                }
                if fetched.is_empty() {
                    return;
                }
                let _ = slint::invoke_from_event_loop(move || {
                    UI.with(|uu| {
                        if let Some((app, weak)) = &*uu.borrow() {
                            for (url, path) in &fetched {
                                if let Ok(img) = Image::load_from_path(Path::new(path)) {
                                    app.hero_cache.borrow_mut().insert(url.clone(), img);
                                }
                            }
                            if let Some(w) = weak.upgrade() {
                                if w.get_tv_visible() {
                                    tv_hero(app, &w);
                                }
                            }
                        }
                    });
                });
            });
        }
    });

    win.on_tv_input({
        let app = app.clone();
        let weak = win.as_weak();
        move |b| {
            if let Some(w) = weak.upgrade() {
                tv_input(&app, &w, b.as_str());
            }
        }
    });

    win.on_refresh(|| ui_refresh());

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
            match actions::installed_dir(&app.paths, &id) {
                Some(dir) => {
                    let _ = open::that(dir);
                }
                None => {}
            }
        }
    });

    win.on_open_game({
        let app = app.clone();
        let handle = handle.clone();
        move |id| {
            let id = id.to_string();
            let Some(project) = app.find(&id) else { return };
            *app.detail.borrow_mut() = Some(DetailState { id: id.clone(), wiki: None, msg: String::new() });
            ui_refresh();
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
        move || {
            *app.detail.borrow_mut() = None;
            ui_refresh();
        }
    });

    win.on_install({
        let app = app.clone();
        let handle = handle.clone();
        move |id| {
            let id = id.to_string();
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
                        ui_refresh();
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

    win.on_cancel_install({
        let app = app.clone();
        move |id| {
            if let Some(flag) = app.cancels.borrow().get(id.as_str()) {
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
            }
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
            if let Some(w) = weak.upgrade() {
                w.set_bulk_busy(true);
                w.set_bulk_progress(0.0);
                w.set_bulk_label(format!("0/{total}").into());
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
                                if let Some((app, _)) = &*u.borrow() {
                                    app.cancels.borrow_mut().insert(pidc, c);
                                }
                            });
                        });
                    }
                    let game = if p.original_game.is_empty() { p.name.clone() } else { p.original_game.clone() };
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
                        let label = format!("{}/{} · {}", i + 1, total, game);
                        let _ = slint::invoke_from_event_loop(move || {
                            UI.with(|u| {
                                if let Some((app, weak)) = &*u.borrow() {
                                    app.install_progress.borrow_mut().insert(idc, f);
                                    if let Some(w) = weak.upgrade() {
                                        w.set_bulk_progress(overall);
                                        w.set_bulk_label(label.into());
                                    }
                                }
                            });
                            ui_refresh();
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
                                if err.as_ref().map(|e| !e.is_cancelled()).unwrap_or(false) {
                                    app.install_error.borrow_mut().insert(pid.clone());
                                }
                                if let Some(w) = weak.upgrade() {
                                    w.set_bulk_progress(done);
                                }
                            }
                        });
                        ui_refresh();
                    });
                }
                // Summary stays visible under the button when something failed
                // (silent per-card ⚠ alone was easy to miss).
                let summary = if failed > 0 {
                    format!("{done_ok} al día · {failed} con error — reintenta desde la tarjeta")
                } else {
                    String::new()
                };
                let _ = slint::invoke_from_event_loop(move || {
                    UI.with(|u| {
                        if let Some((app, weak)) = &*u.borrow() {
                            *app.bulk_cancel.borrow_mut() = None;
                            if let Some(w) = weak.upgrade() {
                                w.set_bulk_busy(false);
                                w.set_bulk_label(summary.into());
                            }
                        }
                    });
                    ui_refresh();
                });
                refresh_library_size(paths.clone()).await;
            });
        }
    });

    win.on_play({
        let app = app.clone();
        move |id| {
            let id = id.to_string();
            let Some(project) = app.find(&id) else { return };
            app.launching.borrow_mut().insert(id.clone(), false); // "Jugando…"
            ui_refresh();
            if actions::launch_project(&app.paths, &project).is_err() {
                app.launching.borrow_mut().insert(id.clone(), true); // failed
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
        }
    });

    win.on_uninstall({
        let app = app.clone();
        let handle = handle.clone();
        move |id| {
            let _ = actions::uninstall_project(&app.paths, &id);
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

    // Land on the library when there is one; the catalog is the storefront,
    // but day-to-day you open the app to play what you already installed.
    if !store::load_installed(&app.paths).unwrap_or_default().is_empty() {
        win.set_library_tab(true);
    }
    if std::env::var_os("FREEPORT_DEBUG_CATALOG").is_some() {
        win.set_library_tab(false); // dev aid: land on the catalog tab
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

    // Card size from config.
    apply_card_size(&win, &store::load_config(&app.paths).map(|c| c.card_size).unwrap_or_default());

    win.on_set_card_size({
        let app = app.clone();
        let weak = win.as_weak();
        move |s| {
            let s = s.to_string();
            if let Ok(mut cfg) = store::load_config(&app.paths) {
                cfg.card_size = s.clone();
                let _ = store::save_config(&app.paths, &cfg);
            }
            if let Some(w) = weak.upgrade() {
                apply_card_size(&w, &s);
            }
            ui_refresh();
        }
    });

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
            if let Ok(cat) = actions::refresh_catalog(&client, &paths, url.as_deref()).await {
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
                    let _ = e;
                    let _ = slint::invoke_from_event_loop(move || {
                        UI.with(|u| {
                            if let Some((_, weak)) = &*u.borrow() {
                                if let Some(w) = weak.upgrade() {
                                    w.set_update_busy(false);
                                    w.set_update_available(true); // keep banner to retry
                                    w.set_update_error(true);
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

    // Windows-only fine polish: rounded corners, shadow and Snap Layouts.
    // Deferred so the winit window exists before we reach for its HWND.
    #[cfg(windows)]
    {
        let weak = win.as_weak();
        slint::Timer::single_shot(std::time::Duration::from_millis(120), move || {
            if let Some(w) = weak.upgrade() {
                w.window().with_winit_window(|ww| {
                    use i_slint_backend_winit::winit::raw_window_handle::{
                        HasWindowHandle, RawWindowHandle,
                    };
                    if let Ok(handle) = ww.window_handle() {
                        if let RawWindowHandle::Win32(h) = handle.as_raw() {
                            win_titlebar::setup(h.hwnd.get());
                        }
                    }
                });
            }
        });
    }

    // `--tv`: boot straight into TV (Big Picture) mode, e.g. when launched by
    // Sunshine for streaming to a couch/TV client.
    if std::env::args().any(|a| a == "--tv") {
        build_tv(&app, &win);
        win.set_tv_visible(true);
        let _ = win.window().set_fullscreen(true);
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
    std::process::exit(0);
}
