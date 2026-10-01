use crate::error::{AppError, AppResult};
use crate::model::{Catalog, Config, InstalledEntry, Project, SystemInfo};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Resolves all on-disk locations the app uses, honoring portable mode.
#[derive(Debug, Clone)]
pub struct Paths {
    pub data_dir: PathBuf,
}

impl Paths {
    /// If a `portable.txt` marker sits next to the executable, all state lives
    /// in a `data/` folder beside the binary; otherwise it uses the OS data dir.
    pub fn resolve() -> AppResult<Self> {
        let data_dir = if let Some(portable) = portable_data_dir() {
            portable
        } else {
            let base = dirs::data_dir()
                .ok_or_else(|| AppError::msg("no se pudo determinar el directorio de datos"))?;
            base.join("decompdeck")
        };
        std::fs::create_dir_all(&data_dir)?;
        std::fs::create_dir_all(data_dir.join("apps"))?;
        std::fs::create_dir_all(data_dir.join("cover_cache"))?;
        Ok(Self { data_dir })
    }

    pub fn installed_file(&self) -> PathBuf {
        self.data_dir.join("installed.json")
    }
    pub fn config_file(&self) -> PathBuf {
        self.data_dir.join("config.json")
    }
    pub fn mod_state_file(&self) -> PathBuf {
        self.data_dir.join("mod_state.json")
    }
    pub fn catalog_cache_file(&self) -> PathBuf {
        self.data_dir.join("catalog_cache.json")
    }
    /// Install directory for a given project id.
    pub fn app_dir(&self, project_id: &str) -> PathBuf {
        self.data_dir.join("apps").join(project_id)
    }
    /// Cache directory for downscaled cover thumbnails (served via `cover://`).
    pub fn cover_cache_dir(&self) -> PathBuf {
        self.data_dir.join("cover_cache")
    }

    pub fn is_portable(&self) -> bool {
        portable_data_dir().is_some()
    }
}

fn portable_data_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    if dir.join("portable.txt").exists() {
        Some(dir.join("data"))
    } else {
        None
    }
}

fn read_json<T: serde::de::DeserializeOwned + Default>(path: &Path) -> AppResult<T> {
    if !path.exists() {
        return Ok(T::default());
    }
    let bytes = std::fs::read(path)?;
    if bytes.is_empty() {
        return Ok(T::default());
    }
    Ok(serde_json::from_slice(&bytes)?)
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> AppResult<()> {
    let data = serde_json::to_vec_pretty(value)?;
    // Write atomically via a temp file next to the target.
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &data)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub type InstalledMap = HashMap<String, InstalledEntry>;

pub fn load_installed(paths: &Paths) -> AppResult<InstalledMap> {
    read_json(&paths.installed_file())
}
pub fn save_installed(paths: &Paths, map: &InstalledMap) -> AppResult<()> {
    write_json(&paths.installed_file(), map)
}

pub fn load_config(paths: &Paths) -> AppResult<Config> {
    read_json(&paths.config_file())
}

/// What DecompDeck installed for one mod: its version and the files it wrote.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct InstalledMod {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub files: Vec<String>,
}

/// Per-project record keyed by mod full_name, so the UI can mark mods as
/// installed / updatable and remove them cleanly.
pub type ModState = HashMap<String, HashMap<String, InstalledMod>>;

pub fn load_mod_state(paths: &Paths) -> AppResult<ModState> {
    // Tolerate an older/mismatched on-disk format by resetting to empty.
    Ok(read_json(&paths.mod_state_file()).unwrap_or_default())
}
pub fn save_mod_state(paths: &Paths, state: &ModState) -> AppResult<()> {
    write_json(&paths.mod_state_file(), state)
}
pub fn save_config(paths: &Paths, cfg: &Config) -> AppResult<()> {
    write_json(&paths.config_file(), cfg)
}

/// Loads the catalog, preferring a previously fetched cache and falling back to
/// the seed bundled into the binary.
pub fn load_catalog(paths: &Paths) -> AppResult<Catalog> {
    let cache = paths.catalog_cache_file();
    let mut cat = None;
    if cache.exists() {
        if let Ok(c) = read_json::<Catalog>(&cache) {
            if !c.projects.is_empty() {
                cat = Some(c);
            }
        }
    }
    let mut cat = match cat {
        Some(c) => c,
        None => serde_json::from_str(include_str!("../catalog.seed.json"))?,
    };
    attach_orphans(paths, &mut cat);
    Ok(cat)
}

/// System id used for installs whose project is no longer in the catalog.
pub const ORPHAN_SYSTEM: &str = "legacy";

/// Keeps installed games visible after the catalog drops them (a project
/// retired from the catalog must not vanish from the user's library with its
/// folder still on disk). Each such install gets a minimal synthetic project:
/// launchable through the binary heuristic, uninstallable, nothing else.
pub fn attach_orphans(paths: &Paths, cat: &mut Catalog) {
    let Ok(installed) = load_installed(paths) else { return };
    let known: std::collections::HashSet<&str> = cat.projects.iter().map(|p| p.id.as_str()).collect();
    let mut orphans: Vec<Project> = Vec::new();
    for (id, entry) in installed.iter() {
        if known.contains(id.as_str()) {
            continue;
        }
        if let Some(p) = orphan_project(id, entry) {
            orphans.push(p);
        }
    }
    if orphans.is_empty() {
        return;
    }
    if !cat.systems.iter().any(|s| s.id == ORPHAN_SYSTEM) {
        cat.systems.push(SystemInfo {
            id: ORPHAN_SYSTEM.into(),
            name: "Fuera del catálogo".into(),
            short: "—".into(),
            color: "#777b86".into(),
        });
    }
    orphans.sort_by(|a, b| a.id.cmp(&b.id));
    cat.projects.extend(orphans);
}

fn orphan_project(id: &str, entry: &InstalledEntry) -> Option<Project> {
    // Human-ish title from the id: "sonic-cd-decomp" → "Sonic Cd Decomp".
    let title: String = id
        .split(['-', '_'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    let v = serde_json::json!({
        "id": id,
        "name": title,
        "original_game": title,
        "system": ORPHAN_SYSTEM,
        "type": "native-port",
        "repo": {"host": "github", "owner": "", "repo": ""},
        "release_channel": "stable",
        "rom": {"required": false, "mode": "none", "notes": "Este juego ya no está en el catálogo de Freeport. Sigue instalado y se puede jugar o eliminar, pero no recibirá actualizaciones."},
        "cached": {"platforms": [if entry.windows { "windows-x86_64" } else { "linux-x86_64" }], "latest_tag": entry.installed_tag, "published_at": entry.published_at}
    });
    serde_json::from_value(v).ok()
}

pub fn save_catalog_cache(paths: &Paths, cat: &Catalog) -> AppResult<()> {
    write_json(&paths.catalog_cache_file(), cat)
}

#[cfg(test)]
mod orphan_tests {
    use super::*;

    #[test]
    fn orphan_project_parses_and_is_launchable_shape() {
        let e: InstalledEntry = serde_json::from_str(
            r#"{"installed_tag":"v1.2","install_path":"/tmp/x","windows":false,"installed_at":"1","published_at":"2026-01-01T00:00:00Z"}"#,
        )
        .unwrap();
        let p = orphan_project("sonic-cd-decomp", &e).expect("synthetic project must deserialize");
        assert_eq!(p.id, "sonic-cd-decomp");
        assert_eq!(p.original_game, "Sonic Cd Decomp");
        assert_eq!(p.system, ORPHAN_SYSTEM);
        assert_eq!(p.rom.mode, "none");
        assert_eq!(p.cached.as_ref().unwrap().latest_tag.as_deref(), Some("v1.2"));
        assert!(p.launch.is_empty()); // heuristic binary lookup
    }
}
