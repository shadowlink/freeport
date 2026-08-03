// Syncs the embedded fallback catalog (src-tauri/catalog.seed.json) from the
// canonical freeport-catalog repo, so release builds ship the freshest catalog.
// Best-effort: on any failure (offline, bad JSON, HTTP error) it keeps the
// existing seed and never fails the build.
//
// Runs automatically as the npm `prebuild` hook (before `npm run build`, which
// Tauri invokes via beforeBuildCommand). Run manually with: node tools/sync-seed.mjs

import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const CATALOG_URL =
  "https://raw.githubusercontent.com/shadowlink/freeport-catalog/main/catalog.json";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const seedPath = join(root, "src-tauri", "catalog.seed.json");

async function main() {
  let remote;
  try {
    const res = await fetch(CATALOG_URL, { signal: AbortSignal.timeout(15000) });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    remote = await res.json();
  } catch (e) {
    console.warn(`[sync-seed] no se pudo obtener el catálogo remoto (${e.message}); se mantiene el seed embebido.`);
    return; // keep existing seed
  }

  if (!remote || !Array.isArray(remote.projects) || remote.projects.length === 0) {
    console.warn("[sync-seed] el catálogo remoto no es válido o está vacío; se mantiene el seed embebido.");
    return;
  }

  const next = JSON.stringify(remote, null, 2) + "\n";
  let current = null;
  try {
    current = await readFile(seedPath, "utf8");
  } catch {
    /* first run / missing seed */
  }

  if (current === next) {
    console.log(`[sync-seed] seed ya al día (${remote.projects.length} juegos).`);
    return;
  }

  await writeFile(seedPath, next);
  console.log(`[sync-seed] seed actualizado desde freeport-catalog (${remote.projects.length} juegos).`);
}

main().catch((e) => {
  // Never fail the build on a sync problem.
  console.warn(`[sync-seed] aviso: ${e?.message ?? e}`);
});
