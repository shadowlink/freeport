import { useEffect, useState } from "react";
import { api } from "../api";
import type { Update } from "../lib/updater";
import type { Runner, SunshineStatus } from "../types";

export default function Settings({
  onClose,
  onChanged,
  version,
  onCheckUpdate,
}: {
  onClose: () => void;
  onChanged: () => void;
  version: string;
  onCheckUpdate: () => Promise<Update | null>;
}) {
  const [showWindows, setShowWindows] = useState(false);
  const [runners, setRunners] = useState<Runner[]>([]);
  const [runner, setRunner] = useState<string>("");
  const [sunshine, setSunshine] = useState<SunshineStatus | null>(null);
  const [sunMsg, setSunMsg] = useState<string | null>(null);
  const [updMsg, setUpdMsg] = useState<string | null>(null);
  const [updBusy, setUpdBusy] = useState(false);
  const [catMsg, setCatMsg] = useState<string | null>(null);

  useEffect(() => {
    api.getConfig().then((c) => {
      setShowWindows(c.show_windows ?? false);
      setRunner(c.wine_runner ?? "");
    });
    api.listRunners().then(setRunners).catch(() => {});
    api.sunshineStatus().then(setSunshine).catch(() => {});
  }, []);

  async function changeRunner(v: string) {
    setRunner(v);
    await api.setRunner(v || null);
  }

  async function toggleWindows(v: boolean) {
    setShowWindows(v);
    await api.setShowWindows(v);
    onChanged();
  }

  async function addSunshine() {
    try {
      setSunMsg(await api.addToSunshine());
      api.sunshineStatus().then(setSunshine).catch(() => {});
    } catch (e) {
      setSunMsg(String(e));
    }
  }

  async function checkApp() {
    setUpdBusy(true);
    setUpdMsg(null);
    try {
      const u = await onCheckUpdate();
      if (u) onClose(); // el banner se encarga de preguntar e instalar
      else setUpdMsg("Ya tienes la última versión.");
    } catch (e) {
      setUpdMsg(String(e));
    } finally {
      setUpdBusy(false);
    }
  }

  async function refreshCatalog() {
    setCatMsg("Actualizando…");
    try {
      await api.refreshCatalog();
      onChanged();
      setCatMsg("Catálogo actualizado.");
    } catch (e) {
      setCatMsg(String(e));
    }
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
        <div className="flex items-baseline justify-between">
          <h2 className="text-xl font-black">Ajustes</h2>
          <span className="text-xs text-white/40">Freeport v{version || "…"}</span>
        </div>

        {/* Juegos de Windows */}
        <div className="rounded-lg border border-edge bg-panel-2 p-3">
          <label className="flex items-center gap-3 cursor-pointer">
            <input
              type="checkbox"
              checked={showWindows}
              onChange={(e) => toggleWindows(e.target.checked)}
              className="w-4 h-4 accent-[color:var(--color-neon)]"
            />
            <span className="font-semibold text-sm">
              Mostrar juegos de Windows (Wine/Proton)
            </span>
          </label>
          {showWindows && (
            <label className="block mt-3 text-sm">
              <span className="text-white/70">Runner</span>
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
              {runners.length === 0 && (
                <span className="block text-[11px] text-hot mt-1">
                  No se detectó Wine ni Proton.
                </span>
              )}
            </label>
          )}
        </div>

        {/* Sunshine / Moonlight */}
        <div className="rounded-lg border border-edge bg-panel-2 p-3">
          <div className="flex items-center justify-between gap-3">
            <span className="font-semibold text-sm">Sunshine / Moonlight</span>
            {sunshine?.found && !sunshine.added && (
              <button
                onClick={addSunshine}
                className="text-sm rounded-md border border-neon/40 text-neon px-3 py-1.5 hover:bg-neon/10"
              >
                Añadir a Sunshine
              </button>
            )}
          </div>
          {sunshine?.found
            ? sunshine.added && (
                <p className="text-[12px] text-white/45 mt-1">
                  Añadido. Ábrelo desde Moonlight en Modo TV.
                </p>
              )
            : (
              <p className="text-[12px] text-white/45 mt-1">No detectado.</p>
            )}
          {sunMsg && <div className="mt-2 text-[12px] text-gold">{sunMsg}</div>}
        </div>

        {/* Actualizaciones */}
        <div className="rounded-lg border border-edge bg-panel-2 p-3">
          <div className="font-semibold text-sm mb-2">Actualizaciones</div>
          <div className="flex flex-wrap gap-2">
            <button
              onClick={checkApp}
              disabled={updBusy}
              className="text-sm rounded-md border border-neon/40 text-neon px-3 py-1.5 hover:bg-neon/10 disabled:opacity-50"
            >
              {updBusy ? "Comprobando…" : "Buscar actualización"}
            </button>
            <button
              onClick={refreshCatalog}
              className="text-sm rounded-md border border-edge px-3 py-1.5 hover:border-neon/50"
            >
              Actualizar catálogo
            </button>
          </div>
          {updMsg && <div className="mt-2 text-[12px] text-gold">{updMsg}</div>}
          {catMsg && <div className="mt-1 text-[12px] text-gold">{catMsg}</div>}
        </div>

        <div className="flex justify-end pt-1">
          <button
            onClick={onClose}
            className="rounded-lg bg-neon text-void font-bold px-4 py-2 hover:brightness-110"
          >
            Cerrar
          </button>
        </div>
      </div>
    </div>
  );
}
