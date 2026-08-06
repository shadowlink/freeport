use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The full catalog document (seed-bundled or fetched from the remote repo).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Catalog {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub systems: Vec<SystemInfo>,
    #[serde(default)]
    pub projects: Vec<Project>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub short: String,
    #[serde(default)]
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub original_game: String,
    pub system: String,
    /// "recompilation" | "native-port"
    #[serde(rename = "type", default)]
    pub kind: String,
    pub repo: RepoRef,
    #[serde(default)]
    pub release_channel: String,
    #[serde(default)]
    pub rolling_tag: Option<String>,
    #[serde(default)]
    pub cover_url: Option<String>,
    /// Preferred boxart (e.g. from ScreenScraper, baked by the catalog tool).
    /// When present the app uses it for the cover; `cover_url` still drives the
    /// libretro-derived screenshots.
    #[serde(default)]
    pub box_art: Option<String>,
    #[serde(default)]
    pub logo_url: Option<String>,
    // Curated "museum" metadata for the game page.
    #[serde(default)]
    pub year: Option<u32>,
    #[serde(default)]
    pub developer: Option<String>,
    #[serde(default)]
    pub genre: Option<String>,
    /// Wikipedia article title used to fetch a description/history at runtime.
    #[serde(default)]
    pub wiki: Option<String>,
    /// Optional mod source (e.g. a Thunderstore community) for one-click mods.
    #[serde(default)]
    pub mods: Option<ModSource>,
    /// platform triple -> regex used to pick the matching release asset.
    #[serde(default)]
    pub asset_rules: HashMap<String, String>,
    #[serde(default)]
    pub rom: RomInfo,
    /// os ("linux"|"windows"|"macos") -> optional binary name hint.
    #[serde(default)]
    pub launch: HashMap<String, Option<String>>,
    #[serde(default)]
    pub cached: Option<Cached>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModSource {
    /// "thunderstore"
    pub source: String,
    /// Thunderstore community slug, e.g. "zelda-64-recompiled".
    pub community: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoRef {
    #[serde(default = "default_host")]
    pub host: String,
    pub owner: String,
    pub repo: String,
}

fn default_host() -> String {
    "github".to_string()
}

impl RepoRef {
    pub fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RomInfo {
    #[serde(default)]
    pub required: bool,
    /// How the port consumes the ROM:
    /// - "in-app": the port has its own first-run ROM picker; DecompDeck does not
    ///   manage the ROM (the user just launches and follows the game's wizard).
    /// - "copy": DecompDeck copies the ROM into the game folder so the port finds
    ///   it (optionally under `expected_filename` and/or `subdir`).
    #[serde(default)]
    pub mode: String,
    /// Exact filename the port requires (e.g. Perfect Dark's pd.ntsc-final.z64).
    /// When absent, the ROM keeps its original name.
    #[serde(default)]
    pub expected_filename: Option<String>,
    /// Subfolder (relative to the launch binary) the ROM must live in, e.g.
    /// Perfect Dark's `data`. When absent, the ROM sits beside the binary.
    #[serde(default)]
    pub subdir: Option<String>,
    #[serde(default)]
    pub notes: String,
}

/// Fields normally refreshed by the catalog repo's CI probe, letting the app
/// pre-filter by platform and show "update available" without hitting GitHub.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Cached {
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub latest_tag: Option<String>,
    #[serde(default)]
    pub published_at: Option<String>,
}

/// Per-project local install record, persisted in `installed.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstalledEntry {
    #[serde(default)]
    pub installed_tag: Option<String>,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub install_path: String,
    #[serde(default)]
    pub rom_path: Option<String>,
    #[serde(default)]
    pub installed_at: Option<String>,
    /// True when this is a Windows build installed to run via Wine/Proton.
    #[serde(default)]
    pub windows: bool,
    /// Unix epoch (secs) of the last launch.
    #[serde(default)]
    pub last_played: Option<String>,
    /// Accumulated play time in seconds.
    #[serde(default)]
    pub play_secs: u64,
}

/// Persisted user configuration (`config.json`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub github_token: Option<String>,
    #[serde(default)]
    pub catalog_url: Option<String>,
    /// When true, games with only a Windows build are shown and can be
    /// installed to run through Wine/Proton.
    #[serde(default)]
    pub show_windows: bool,
    /// Default runner id for Windows builds (None = auto). See `list_runners`.
    #[serde(default)]
    pub wine_runner: Option<String>,
    /// Per-project runner override (project id -> runner id).
    #[serde(default)]
    pub game_runners: HashMap<String, String>,
    /// repo slug -> last-seen ETag, to make polling cheap (304s don't count
    /// against the rate limit).
    #[serde(default)]
    pub etags: HashMap<String, String>,
    /// Discord Application ID for Rich Presence (public id, not a secret).
    /// None/empty = presence disabled. See `discord.rs`.
    #[serde(default)]
    pub discord_app_id: Option<String>,
    /// Project ids the user marked as favorite.
    #[serde(default)]
    pub favorites: Vec<String>,
    /// Update version the user chose to skip ("Después/omitir").
    #[serde(default)]
    pub skip_version: Option<String>,
    /// Retro CRT screen effect (scanlines + vignette).
    #[serde(default)]
    pub crt: bool,
    /// RetroAchievements account (managed centrally; handed to the RA mod).
    #[serde(default)]
    pub ra_user: Option<String>,
    #[serde(default)]
    pub ra_token: Option<String>,
}

/// Enriched view of a project sent to the frontend: the catalog entry plus its
/// local install status.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectView {
    #[serde(flatten)]
    pub project: Project,
    pub installed: bool,
    pub installed_tag: Option<String>,
    pub update_available: bool,
    /// Whether the user has already linked a ROM for this install (only
    /// meaningful for `rom.mode == "copy"` projects).
    pub rom_configured: bool,
    /// True when only a Windows build is available for this platform (shown
    /// because `show_windows` is on) — it will run via Wine/Proton.
    pub is_windows: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CatalogView {
    pub platform: String,
    pub systems: Vec<SystemInfo>,
    pub projects: Vec<ProjectView>,
}
