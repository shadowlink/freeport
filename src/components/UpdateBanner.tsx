import { useState } from "react";
import { installUpdate, type Update } from "../lib/updater";

// Banner que aparece cuando hay una versión nueva. App decide cuándo mostrarlo
// (pasando `update`); aquí se maneja la descarga + instalación con feedback.
export default function UpdateBanner({
  update,
  onDismiss,
}: {
  update: Update | null;
  onDismiss: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [pct, setPct] = useState<number | null>(null);
  const [err, setErr] = useState<string | null>(null);

  if (!update) return null;

  const run = async () => {
    setBusy(true);
    setErr(null);
    try {
      await installUpdate(update, setPct); // reinicia la app al terminar
    } catch (e) {
      setErr(String(e));
      setBusy(false);
    }
  };

  return (
    <div className="fixed top-10 inset-x-0 z-[60] flex justify-center px-4 pt-2 pointer-events-none">
      <div className="pointer-events-auto w-full max-w-xl rounded-xl border border-neon/50 bg-panel shadow-2xl px-4 py-3 flex items-center gap-3">
        <span className="text-xl">⬆️</span>
        <div className="min-w-0 flex-1">
          <div className="text-sm font-bold">
            Versión nueva disponible: <span className="text-neon">v{update.version}</span>
          </div>
          {busy ? (
            <div className="mt-1.5">
              <div className="h-1.5 rounded bg-panel-2 overflow-hidden">
                <div
                  className="h-full bg-neon transition-[width] duration-200"
                  style={{ width: pct == null ? "40%" : `${pct}%` }}
                />
              </div>
              <div className="text-[11px] text-white/50 mt-1">
                {pct == null ? "Descargando…" : `Descargando ${pct}%`} · se reiniciará al terminar
              </div>
            </div>
          ) : (
            err && <div className="text-[12px] text-hot mt-0.5">{err}</div>
          )}
        </div>
        {!busy && (
          <div className="flex items-center gap-2 shrink-0">
            <button
              onClick={onDismiss}
              className="text-xs rounded-lg border border-edge px-3 py-1.5 text-white/60 hover:text-white"
            >
              Después
            </button>
            <button
              onClick={run}
              className="text-xs rounded-lg bg-neon text-void font-bold px-3 py-1.5 hover:brightness-110"
            >
              Actualizar
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
