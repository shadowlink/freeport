import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api } from "../api";
import type { PathsInfo, Runner, SunshineStatus } from "../types";

export default function Settings({
  onClose,
  onChanged,
}: {
  onClose: () => void;
  onChanged: () => void;
}) {
  const [token, setToken] = useState("");
  const [catalogUrl, setCatalogUrl] = useState("");
  const [discordId, setDiscordId] = useState("");
  const [showWindows, setShowWindows] = useState(false);
  const [paths, setPaths] = useState<PathsInfo | null>(null);
  const [saved, setSaved] = useState(false);
  const [runners, setRunners] = useState<Runner[]>([]);
  const [runner, setRunner] = useState<string>("");
  const [sunshine, setSunshine] = useState<SunshineStatus | null>(null);
  const [sunMsg, setSunMsg] = useState<string | null>(null);

  useEffect(() => {
    api.getConfig().then((c) => {
      setToken(c.github_token ?? "");
      setCatalogUrl(c.catalog_url ?? "");
      setShowWindows(c.show_windows ?? false);
      setRunner(c.wine_runner ?? "");
      setDiscordId(c.discord_app_id ?? "");
    });
    api.getPathsInfo().then(setPaths);
    api.listRunners().then(setRunners).catch(() => {});
    api.sunshineStatus().then(setSunshine).catch(() => {});
  }, []);

  async function changeRunner(v: string) {
    setRunner(v);
    await api.setRunner(v || null);
  }

  async function addSunshine() {
    try {
      setSunMsg(await api.addToSunshine());
      api.sunshineStatus().then(setSunshine).catch(() => {});
    } catch (e) {
      setSunMsg(String(e));
    }
  }

  async function toggleWindows(v: boolean) {
    setShowWindows(v);
    await api.setShowWindows(v);
    onChanged(); // reload the catalog so the change is visible immediately
  }

  async function save() {
    await api.setConfig(token || null, catalogUrl || null);
    await api.setDiscordAppId(discordId || null);
    setSaved(true);
    setTimeout(() => setSaved(false), 1500);
  }

  return (
    <div
      className="fixed inset-0 z-40 bg-black/70 flex items-center justify-center p-6"
      onClick={onClose}
    >
      <div
        className="w-full max-w-md rounded-2xl border border-edge bg-panel p-5 space-y-4"
        onClick={(e) => e.stopPropagation()}
      >
        <h2 className="text-xl font-black">Ajustes</h2>

        {/* Windows builds via Wine/Proton */}
        <div className="rounded-lg border border-edge bg-panel-2 p-3">
          <label className="flex items-start gap-3 cursor-pointer">
            <input
              type="checkbox"
              checked={showWindows}
              onChange={(e) => toggleWindows(e.target.checked)}
              className="mt-0.5 w-4 h-4 accent-[color:var(--color-neon)]"
            />
            <span>
              <span className="font-semibold text-sm">
                Mostrar versiones de Windows
              </span>
              <span className="block text-[12px] text-white/55 leading-relaxed mt-0.5">
                Incluye juegos que <b>solo tienen ejecutable de Windows</b>. Se
                descargan y se ejecutan mediante <b>Wine/Proton</b> (debes tenerlo
                instalado). Aparecen marcados con la etiqueta <b>WIN</b>.
              </span>
            </span>
          </label>

          {showWindows && (
            <label className="block mt-3 text-sm">
              <span className="text-white/70">Runner por defecto</span>
              <select
                value={runner}
                onChange={(e) => changeRunner(e.target.value)}
                className="mt-1 w-full rounded-md bg-panel border border-edge px-2 py-2 text-sm outline-none focus:border-neon/50"
              >
                <option value="">Automático</option>
                {runners.map((r) => (
                  <option key={r.id} value={r.id}>
                    {r.label}
                  </option>
                ))}
              </select>
              <span className="block text-[11px] text-white/40 mt-1">
                {runners.length
                  ? `Detectados: ${runners.map((r) => r.label).join(", ")}`
                  : "No se detectó Wine ni Proton (umu). Instala uno."}
              </span>
            </label>
          )}
        </div>

        <label className="block text-sm">
          <span className="text-white/70">Token de GitHub (opcional)</span>
          <input
            type="password"
            value={token}
            onChange={(e) => setToken(e.target.value)}
            placeholder="ghp_… — sube el límite de 60 a 5000 peticiones/hora"
            className="mt-1 w-full rounded-md bg-panel-2 border border-edge px-3 py-2 text-sm outline-none focus:border-neon/50"
          />
        </label>

        <label className="block text-sm">
          <span className="text-white/70">URL del catálogo remoto (opcional)</span>
          <input
            value={catalogUrl}
            onChange={(e) => setCatalogUrl(e.target.value)}
            placeholder="https://raw.githubusercontent.com/…/catalog.json"
            className="mt-1 w-full rounded-md bg-panel-2 border border-edge px-3 py-2 text-sm outline-none focus:border-neon/50"
          />
          <span className="block text-[11px] text-white/40 mt-1">
            Vacío = repo oficial <code className="text-white/60">freeport-catalog</code>
            {" "}(se actualiza a diario). La app cae al catálogo embebido si no hay red.
          </span>
        </label>

        {/* Sunshine / Moonlight */}
        <div className="rounded-lg border border-edge bg-panel-2 p-3">
          <div className="font-semibold text-sm mb-1">Sunshine / Moonlight</div>
          {sunshine?.found ? (
            <>
              <p className="text-[12px] text-white/55 leading-relaxed">
                {sunshine.added
                  ? "Freeport ya está registrado en Sunshine. Ábrelo desde Moonlight y se lanzará en Modo TV."
                  : "Añade Freeport a Sunshine para lanzarlo por streaming (Moonlight) en Modo TV a pantalla completa."}
              </p>
              {!sunshine.added && (
                <button
                  onClick={addSunshine}
                  className="mt-2 text-sm rounded-md border border-neon/40 text-neon px-3 py-1.5 hover:bg-neon/10"
                >
                  Añadir a Sunshine
                </button>
              )}
            </>
          ) : (
            <p className="text-[12px] text-white/45">
              No se detectó Sunshine (apps.json). Instálalo para poder jugar en la TV vía Moonlight.
            </p>
          )}
          {sunMsg && <div className="mt-2 text-[12px] text-gold">{sunMsg}</div>}
        </div>

        {/* Discord Rich Presence */}
        <div className="rounded-lg border border-edge bg-panel-2 p-3">
          <div className="font-semibold text-sm mb-1">Discord Rich Presence</div>
          <p className="text-[12px] text-white/55 leading-relaxed mb-2">
            Muestra <b>«Jugando &lt;juego&gt;»</b> en tu estado de Discord mientras
            juegas. Necesitas crear una app gratis en el{" "}
            <button
              onClick={() =>
                openUrl("https://discord.com/developers/applications")
              }
              className="underline text-neon hover:text-white/80"
            >
              Discord Developer Portal
            </button>
            , copiar su <b>Application ID</b> y (en <b>Rich Presence → Art Assets</b>)
            subir el logo con la clave <code className="text-white/70">freeport</code>.
          </p>
          <input
            value={discordId}
            onChange={(e) => setDiscordId(e.target.value)}
            placeholder="Application ID (solo dígitos) — vacío = desactivado"
            className="w-full rounded-md bg-panel border border-edge px-3 py-2 text-sm outline-none focus:border-neon/50"
          />
        </div>

        {paths && (
          <div className="text-xs text-white/50 rounded-md border border-edge bg-panel-2 p-2">
            <div>
              Datos: <code className="text-white/70">{paths.data_dir}</code>
            </div>
            <div>Modo portable: {paths.portable ? "sí" : "no"}</div>
          </div>
        )}

        <div className="flex justify-end gap-2 pt-1">
          <button
            onClick={onClose}
            className="rounded-lg border border-edge text-white/70 px-4 py-2 hover:border-neon/40"
          >
            Cerrar
          </button>
          <button
            onClick={save}
            className="rounded-lg bg-neon text-void font-bold px-4 py-2 hover:brightness-110"
          >
            {saved ? "Guardado ✓" : "Guardar"}
          </button>
        </div>
      </div>
    </div>
  );
}
