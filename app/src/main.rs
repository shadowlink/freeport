// Freeport — native UI (Slint) on top of freeport-core (reused backend).
// Phase 1-2: catalog view + install/launch/rom/uninstall + on-demand thumbnails.

slint::include_modules!();

use freeport_core::model::{Catalog, Project};
use freeport_core::store::{self, Paths};
use freeport_core::mods::ModInfo;
use freeport_core::wiki::WikiInfo;
use freeport_core::{actions, gamepad, platform, thumbs, update, wiki};
use slint::{Color, Image, ModelRc, SharedString, VecModel, Weak};
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::Path;
use std::rc::Rc;

const LOGOS: &[(&str, &[u8])] = &[
    ("amiga", include_bytes!("../assets/logos/amiga.svg")),
    ("gb", include_bytes!("../assets/logos/gb.svg")),
    ("gba", include_bytes!("../assets/logos/gba.svg")),
    ("gc", include_bytes!("../assets/logos/gc.svg")),
    ("genesis", include_bytes!("../assets/logos/genesis.svg")),
    ("n64", include_bytes!("../assets/logos/n64.svg")),
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
}

struct App {
    catalog: RefCell<Catalog>,
    triple: String,
    paths: Paths,
    client: reqwest::Client,
    logos: std::collections::HashMap<String, Image>,
    runners: Vec<actions::Runner>,
    busy: RefCell<HashSet<String>>,
    detail: RefCell<Option<DetailState>>,
    mods_cache: RefCell<std::collections::HashMap<String, Vec<ModInfo>>>,
    mod_busy: RefCell<HashSet<String>>,
    screens_cache: RefCell<std::collections::HashMap<String, Vec<String>>>,
    tv_shelves: RefCell<Vec<(String, Vec<String>)>>,
    pending_update: RefCell<Option<update::Update>>,
}

thread_local! {
    static UI: RefCell<Option<(Rc<App>, Weak<MainWindow>)>> = const { RefCell::new(None) };
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
    let busy = app.busy.borrow();
    let catalog = app.catalog.borrow();
    let show_windows = store::load_config(&app.paths).map(|c| c.show_windows).unwrap_or(false);

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

    let mut cards: Vec<CardItem> = Vec::new();
    for p in &catalog.projects {
        let (visible, is_win) = app.visibility(p, &installed, show_windows);
        if !visible
            || (library && !installed.contains_key(&p.id))
            || (!active.is_empty() && p.system != active)
        {
            continue;
        }
        if !query.is_empty()
            && !format!("{} {}", p.original_game, p.name).to_lowercase().contains(&query)
        {
            continue;
        }
        let entry = installed.get(&p.id);
        let update = match (entry.and_then(|e| e.installed_tag.as_ref()), &p.cached) {
            (Some(cur), Some(c)) => c.latest_tag.as_ref().map(|l| l != cur).unwrap_or(false),
            _ => false,
        };
        let sys_color = catalog
            .systems
            .iter()
            .find(|s| s.id == p.system)
            .map(|s| parse_color(&s.color))
            .unwrap_or(Color::from_rgb_u8(0x88, 0x88, 0x88));
        let title = if p.original_game.is_empty() { p.name.clone() } else { p.original_game.clone() };
        cards.push(CardItem {
            id: p.id.clone().into(),
            title: title.into(),
            subtitle: p.name.clone().into(),
            cover: app
                .cover(p),
            installed: entry.is_some(),
            is_windows: is_win,
            update_available: update,
            needs_rom: p.rom.mode == "copy",
            rom_ok: entry.and_then(|e| e.rom_path.as_ref()).is_some(),
            busy: busy.contains(&p.id),
            kind: if p.kind == "recompilation" { "RECOMP" } else { "PORT" }.into(),
            sys_color,
        });
    }

    let count = cards.len() as i32;
    let rows: Vec<ModelRc<CardItem>> = cards
        .chunks(6)
        .map(|c| ModelRc::new(VecModel::from(c.to_vec())))
        .collect();

    let header = if !active.is_empty() {
        catalog.systems.iter().find(|s| s.id == active).map(|s| s.name.clone()).unwrap_or(active)
    } else if library {
        "Mi biblioteca".to_string()
    } else {
        "Catálogo".to_string()
    };

    win.set_systems(ModelRc::new(VecModel::from(sys_rows)));
    win.set_rows(ModelRc::new(VecModel::from(rows)));
    win.set_header_title(header.into());
    win.set_header_count(count);
}

impl App {
    fn cover(&self, p: &Project) -> Image {
        p.cover_url
            .as_ref()
            .map(|u| thumbs::path_for(&self.paths, u))
            .filter(|path| path.exists())
            .and_then(|path| Image::load_from_path(&path).ok())
            .unwrap_or_default()
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
    let update = match (entry.and_then(|e| e.installed_tag.as_ref()), &p.cached) {
        (Some(cur), Some(c)) => c.latest_tag.as_ref().map(|l| l != cur).unwrap_or(false),
        _ => false,
    };
    let sys = catalog.systems.iter().find(|s| s.id == p.system);
    let sys_color = sys.map(|s| parse_color(&s.color)).unwrap_or(Color::from_rgb_u8(0x88, 0x88, 0x88));
    let neutral = Color::from_rgb_u8(0x3a, 0x3f, 0x4d);

    let mut chips: Vec<ChipData> = Vec::new();
    if let Some(s) = sys {
        chips.push(ChipData { text: s.name.clone().into(), color: sys_color });
    }
    chips.push(ChipData {
        text: if p.kind == "recompilation" { "Recompilación" } else { "Port nativo" }.into(),
        color: neutral,
    });
    if let Some(y) = p.year {
        chips.push(ChipData { text: format!("{y}").into(), color: neutral });
    }
    if let Some(d) = &p.developer {
        chips.push(ChipData { text: d.clone().into(), color: neutral });
    }
    if let Some(g) = &p.genre {
        chips.push(ChipData { text: g.clone().into(), color: neutral });
    }
    if is_win {
        chips.push(ChipData { text: "Windows · Wine/Proton".into(), color: parse_color("#2b6fb3") });
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
            kind: "".into(),
            sys_color,
        })
        .collect();

    let mut mod_rows: Vec<ModRow> = Vec::new();
    if p.mods.is_some() {
        if let Some(list) = app.mods_cache.borrow().get(&p.id) {
            let inst = actions::installed_mods(&app.paths, &p.id);
            let mbusy = app.mod_busy.borrow();
            for m in list.iter().take(80) {
                let iv = inst.get(&m.full_name);
                mod_rows.push(ModRow {
                    full_name: m.full_name.clone().into(),
                    name: m.name.replace('_', " ").into(),
                    owner: m.owner.clone().into(),
                    downloads: format!("{}", m.downloads).into(),
                    description: m.description.clone().into(),
                    installed: iv.is_some(),
                    update: iv.map(|v| v != &m.version).unwrap_or(false),
                    busy: mbusy.contains(&m.full_name),
                });
            }
        }
    }

    let mut screens: Vec<ScreenShot> = Vec::new();
    if let Some(list) = app.screens_cache.borrow().get(&p.id) {
        for path in list {
            let img = Image::load_from_path(Path::new(path)).unwrap_or_default();
            screens.push(ScreenShot { img, path: path.clone().into() });
        }
    }

    let title = if p.original_game.is_empty() { p.name.clone() } else { p.original_game.clone() };
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
        about: state.wiki.as_ref().map(|w| w.extract.clone()).unwrap_or_default().into(),
        port_notes: p.rom.notes.clone().into(),
        has_related: !related.is_empty(),
        has_mods: !mod_rows.is_empty(),
        has_screens: !screens.is_empty(),
        chips: ModelRc::new(VecModel::from(chips)),
        related: ModelRc::new(VecModel::from(related)),
        mods: ModelRc::new(VecModel::from(mod_rows)),
        screens: ModelRc::new(VecModel::from(screens)),
    };
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

/// Generate any not-yet-cached cover thumbnails in the background, then refresh.
fn spawn_missing_thumbs(app: &Rc<App>, handle: &tokio::runtime::Handle) {
    let missing: Vec<String> = app
        .catalog
        .borrow()
        .projects
        .iter()
        .filter_map(|p| p.cover_url.clone())
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
                win.set_tv_hero_cover(app.cover(&p));
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
    let show_windows = store::load_config(&app.paths).map(|c| c.show_windows).unwrap_or(false);
    let catalog = app.catalog.borrow();
    let mut bounds: Vec<(String, Vec<String>)> = Vec::new();
    let mut shelves: Vec<TvShelf> = Vec::new();
    for s in &catalog.systems {
        let games: Vec<&Project> = catalog
            .projects
            .iter()
            .filter(|p| p.system == s.id && app.visibility(p, &installed, show_windows).0)
            .collect();
        if games.is_empty() {
            continue;
        }
        let sys_color = parse_color(&s.color);
        let cards: Vec<CardItem> = games
            .iter()
            .map(|p| CardItem {
                id: p.id.clone().into(),
                title: (if p.original_game.is_empty() { p.name.clone() } else { p.original_game.clone() }).into(),
                subtitle: p.name.clone().into(),
                cover: app.cover(p),
                installed: installed.contains_key(&p.id),
                is_windows: false,
                update_available: false,
                needs_rom: false,
                rom_ok: false,
                busy: false,
                kind: "".into(),
                sys_color,
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
                c = c.min(len_of(s) - 1).max(0);
            }
        }
        "up" => {
            if s > 0 {
                s -= 1;
                c = c.min(len_of(s) - 1).max(0);
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
                        if let Err(e) = actions::launch_project(&app.paths, &p) {
                            eprintln!("[tv] launch {}: {e}", p.id);
                        }
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
        detail: RefCell::new(None),
        mods_cache: RefCell::new(std::collections::HashMap::new()),
        mod_busy: RefCell::new(HashSet::new()),
        screens_cache: RefCell::new(std::collections::HashMap::new()),
        tv_shelves: RefCell::new(Vec::new()),
        pending_update: RefCell::new(None),
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

    // Populate settings UI.
    {
        let cfg = store::load_config(&app.paths).unwrap_or_default();
        win.set_version(env!("CARGO_PKG_VERSION").into());
        win.set_platform_label(app.triple.clone().into());
        win.set_show_windows(cfg.show_windows);
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
        move || {
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
                                    "Catálogo actualizado ✓"
                                }
                                Err(_) => "No se pudo actualizar",
                            };
                            if let Some(w) = weak.upgrade() {
                                w.set_settings_msg(msg.into());
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
        let weak = win.as_weak();
        move || {
            if let Some(w) = weak.upgrade() {
                build_tv(&app, &w);
                w.set_tv_visible(true);
                let _ = w.window().set_fullscreen(true);
            }
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

    win.on_open_game({
        let app = app.clone();
        let handle = handle.clone();
        move |id| {
            let id = id.to_string();
            let Some(project) = app.find(&id) else { return };
            *app.detail.borrow_mut() = Some(DetailState { id: id.clone(), wiki: None });
            ui_refresh();
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
                let proj = project.clone();
                let pid = id.clone();
                handle.spawn(async move {
                    if let Ok(list) = actions::list_mods(&client, &proj).await {
                        let _ = slint::invoke_from_event_loop(move || {
                            UI.with(|u| {
                                if let Some((app, _)) = &*u.borrow() {
                                    app.mods_cache.borrow_mut().insert(pid, list);
                                }
                            });
                            ui_refresh();
                        });
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

    win.on_open_screenshot(|path| {
        let _ = open::that(path.as_str());
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
            ui_refresh();
            let client = app.client.clone();
            let paths = app.paths.clone();
            handle.spawn(async move {
                if let Err(e) = actions::install_mod(&client, &paths, &project, &all, &fname).await {
                    eprintln!("[mod] {fname}: {e}");
                }
                let _ = slint::invoke_from_event_loop(move || {
                    UI.with(|u| {
                        if let Some((app, _)) = &*u.borrow() {
                            app.mod_busy.borrow_mut().remove(&fname);
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
            ui_refresh();
            let client = app.client.clone();
            let paths = app.paths.clone();
            let cfg = store::load_config(&paths).unwrap_or_default();
            handle.spawn(async move {
                if let Err(e) = actions::install_project(&client, &paths, &project, &cfg).await {
                    eprintln!("[install] {id}: {e}");
                }
                let _ = slint::invoke_from_event_loop(move || {
                    UI.with(|u| {
                        if let Some((app, _)) = &*u.borrow() {
                            app.busy.borrow_mut().remove(&id);
                        }
                    });
                    ui_refresh();
                });
            });
        }
    });

    win.on_play({
        let app = app.clone();
        move |id| {
            if let Some(project) = app.find(&id) {
                if let Err(e) = actions::launch_project(&app.paths, &project) {
                    eprintln!("[launch] {id}: {e}");
                }
            }
        }
    });

    win.on_uninstall({
        let app = app.clone();
        move |id| {
            let _ = actions::uninstall_project(&app.paths, &id);
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
                if let Err(e) = actions::set_rom(&app.paths, &project, &file.to_string_lossy()) {
                    eprintln!("[rom] {id}: {e}");
                }
                ui_refresh();
            }
        }
    });

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
            }
            let client = app.client.clone();
            handle.spawn(async move {
                if let Err(e) = update::apply(&client, &upd).await {
                    eprintln!("[update] {e}");
                    let _ = slint::invoke_from_event_loop(|| {
                        UI.with(|u| {
                            if let Some((_, weak)) = &*u.borrow() {
                                if let Some(w) = weak.upgrade() {
                                    w.set_update_busy(false);
                                    w.set_update_available(false);
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
        let weak = win.as_weak();
        move || {
            if let Some(w) = weak.upgrade() {
                w.set_update_available(false);
            }
        }
    });

    // Check for app updates on startup.
    {
        let client = app.client.clone();
        handle.spawn(async move {
            if let Some(upd) = update::check(&client, env!("CARGO_PKG_VERSION")).await {
                let ver = upd.version.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    UI.with(|u| {
                        if let Some((app, weak)) = &*u.borrow() {
                            *app.pending_update.borrow_mut() = Some(upd);
                            if let Some(w) = weak.upgrade() {
                                w.set_update_version(ver.into());
                                w.set_update_available(true);
                            }
                        }
                    });
                });
            }
        });
    }

    println!("[freeport] plataforma {}", app.triple);
    win.run()?;
    Ok(())
}
