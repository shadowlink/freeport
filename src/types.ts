// Mirrors the structs returned by the Rust backend (see src-tauri/src/model.rs).

export interface SystemInfo {
  id: string;
  name: string;
  short: string;
  color: string;
}

export interface RomInfo {
  required: boolean;
  mode: string; // "runtime" | "build"
  expected_filename: string | null;
  notes: string;
}

export interface RepoRef {
  host: string;
  owner: string;
  repo: string;
}

export interface Cached {
  platforms: string[];
  latest_tag: string | null;
  published_at: string | null;
}

export interface ProjectView {
  id: string;
  name: string;
  original_game: string;
  system: string;
  type: string; // "recompilation" | "native-port"
  repo: RepoRef;
  release_channel: string;
  rolling_tag: string | null;
  cover_url: string | null;
  logo_url: string | null;
  year: number | null;
  developer: string | null;
  genre: string | null;
  wiki: string | null;
  mods: { source: string; community: string } | null;
  asset_rules: Record<string, string>;
  rom: RomInfo;
  launch: Record<string, string | null>;
  cached: Cached | null;
  // Enriched fields:
  installed: boolean;
  installed_tag: string | null;
  update_available: boolean;
  rom_configured: boolean;
  is_windows: boolean;
}

export interface CatalogView {
  platform: string;
  systems: SystemInfo[];
  projects: ProjectView[];
}

export interface InstalledEntry {
  installed_tag: string | null;
  published_at: string | null;
  install_path: string;
  rom_path: string | null;
  installed_at: string | null;
}

export interface Config {
  github_token: string | null;
  catalog_url: string | null;
  show_windows: boolean;
  wine_runner: string | null;
  game_runners: Record<string, string>;
  etags: Record<string, string>;
  discord_app_id: string | null;
}

export interface Runner {
  id: string;
  label: string;
  kind: string; // "wine" | "proton"
}

export interface SunshineStatus {
  found: boolean;
  added: boolean;
  path: string | null;
}

export interface ModInfo {
  full_name: string;
  name: string;
  owner: string;
  description: string;
  version: string;
  download_url: string;
  icon_url: string | null;
  downloads: number;
  dependencies: string[];
  package_url: string | null;
}

export interface PathsInfo {
  data_dir: string;
  portable: boolean;
}

export interface InstallProgress {
  id: string;
  phase: "download" | "extract" | "done";
  downloaded: number;
  total: number;
}

export interface UpdateInfo {
  id: string;
  name: string;
  current: string | null;
  latest: string;
  update_available: boolean;
}

export interface WikiInfo {
  title: string;
  extract: string;
  url: string | null;
  thumbnail: string | null;
  lang: string;
}
