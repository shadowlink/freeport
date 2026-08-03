use crate::error::{AppError, AppResult};
use crate::model::{Catalog, Config, InstalledEntry};
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
    if cache.exists() {
        if let Ok(cat) = read_json::<Catalog>(&cache) {
            if !cat.projects.is_empty() {
                return Ok(cat);
            }
        }
    }
    let seed = include_str!("../catalog.seed.json");
    Ok(serde_json::from_str(seed)?)
}

pub fn save_catalog_cache(paths: &Paths, cat: &Catalog) -> AppResult<()> {
    write_json(&paths.catalog_cache_file(), cat)
}
