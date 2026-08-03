// App self-update via the Tauri updater plugin. Checks the release manifest
// (latest.json in the freeport repo), and downloads + installs a signed update,
// then relaunches. All best-effort: returns null / no-ops when unavailable
// (offline, running under `npm run dev`, or no newer version).

import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type { Update };

/** Returns an available update, or null (no update / not available / error). */
export async function checkForUpdate(): Promise<Update | null> {
  try {
    return await check();
  } catch {
    return null;
  }
}

/** Downloads + installs the update (reporting 0–100% or null when unknown),
 *  then relaunches the app. */
export async function installUpdate(
  update: Update,
  onProgress?: (percent: number | null) => void,
): Promise<void> {
  let total = 0;
  let got = 0;
  await update.downloadAndInstall((e) => {
    switch (e.event) {
      case "Started":
        total = e.data.contentLength ?? 0;
        onProgress?.(total ? 0 : null);
        break;
      case "Progress":
        got += e.data.chunkLength;
        onProgress?.(total ? Math.min(100, Math.round((got / total) * 100)) : null);
        break;
      case "Finished":
        onProgress?.(100);
        break;
    }
  });
  await relaunch();
}
