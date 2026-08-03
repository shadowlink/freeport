import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  CatalogView,
  Config,
  InstalledEntry,
  InstallProgress,
  ModInfo,
  PathsInfo,
  Runner,
  SunshineStatus,
  UpdateInfo,
  WikiInfo,
} from "./types";

export const api = {
  getPlatform: () => invoke<string>("get_platform"),
  systemLogo: (id: string) => invoke<string | null>("system_logo", { id }),
  fetchWiki: (title: string) => invoke<WikiInfo | null>("fetch_wiki", { title }),
  listMods: (id: string) => invoke<ModInfo[]>("list_mods", { id }),
  installMod: (id: string, fullName: string) =>
    invoke<string[]>("install_mod", { id, fullName }),
  installedMods: (id: string) => invoke<Record<string, string>>("installed_mods", { id }),
  uninstallMod: (id: string, fullName: string) =>
    invoke<void>("uninstall_mod", { id, fullName }),
  isTvMode: () => invoke<boolean>("is_tv_mode"),
  sunshineStatus: () => invoke<SunshineStatus>("sunshine_status"),
  addToSunshine: () => invoke<string>("add_to_sunshine"),
  getPathsInfo: () => invoke<PathsInfo>("get_paths_info"),
  getConfig: () => invoke<Config>("get_config"),
  setConfig: (github_token: string | null, catalog_url: string | null) =>
    invoke<void>("set_config", { githubToken: github_token, catalogUrl: catalog_url }),
  setShowWindows: (value: boolean) => invoke<void>("set_show_windows", { value }),
  setDiscordAppId: (value: string | null) =>
    invoke<void>("set_discord_app_id", { value }),
  listRunners: () => invoke<Runner[]>("list_runners"),
  setRunner: (runner: string | null) => invoke<void>("set_runner", { runner }),
  setGameRunner: (id: string, runner: string | null) =>
    invoke<void>("set_game_runner", { id, runner }),
  listCatalog: () => invoke<CatalogView>("list_catalog"),
  refreshCatalog: () => invoke<string>("refresh_catalog"),
  installProject: (id: string) => invoke<InstalledEntry>("install_project", { id }),
  uninstallProject: (id: string) => invoke<void>("uninstall_project", { id }),
  setRom: (id: string, romSource: string) =>
    invoke<void>("set_rom", { id, romSource }),
  launchProject: (id: string) => invoke<number>("launch_project", { id }),
  checkUpdates: () => invoke<UpdateInfo[]>("check_updates"),
};

export function onInstallProgress(
  cb: (p: InstallProgress) => void,
): Promise<UnlistenFn> {
  return listen<InstallProgress>("install://progress", (e) => cb(e.payload));
}

export interface ModProgress {
  id: string;
  target: string;
  pkg: string;
  index: number;
  total: number;
  downloaded: number;
  total_bytes: number;
  phase: "download" | "extract" | "done";
}

export function onModProgress(cb: (p: ModProgress) => void): Promise<UnlistenFn> {
  return listen<ModProgress>("mod://progress", (e) => cb(e.payload));
}

/// Fires when a launched game process exits (so the TV UI can re-take focus).
export function onGameExited(cb: (id: string) => void): Promise<UnlistenFn> {
  return listen<{ id: string }>("game://exited", (e) => cb(e.payload.id));
}

/// Semantic controller input from the Rust gilrs reader.
export function onGamepad(cb: (button: string) => void): Promise<UnlistenFn> {
  return listen<{ button: string }>("gamepad://input", (e) => cb(e.payload.button));
}
