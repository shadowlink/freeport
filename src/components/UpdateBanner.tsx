import { useEffect, useState } from "react";
import { checkForUpdate, installUpdate, type Update } from "../lib/updater";

// Shows a banner when a newer Freeport release is available, and handles the
// download + install + relaunch flow. Auto-checks once on startup.
export default function UpdateBanner() {
  const [update, setUpdate] = useState<Update | null>(null);
  const [dismissed, setDismissed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [pct, setPct] = useState<number | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    checkForUpdate().then((u) => u && setUpdate(u));
  }, []);

  if (!update || dismissed) return null;

  const run = async () => {
    setBusy(true);
    setErr(null);
    try {
      await installUpdate(update, setPct); // relaunches on success
    } catch (e) {
      setErr(String(e));
      setBusy(false);
    }
  };

  return (
    <div className="fixed top-0 inset-x-0 z-[60] flex justify-center px-4 pt-3 pointer-events-none">
      <div className="pointer-events-auto w-full max-w-xl rounded-xl border border-neon/50 bg-panel shadow-2xl px-4 py-3 flex items-center gap-3">
        <span className="text-xl">⬆️</span>
        <div className="min-w-0 flex-1">
          <div className="text-sm font-bold">
            Nueva versión de Freeport disponible:{" "}
            <span className="text-neon">v{update.version}</span>
          </div>
          {busy ? (
            <div className="mt-1">
              <div className="h-1.5 rounded bg-panel-2 overflow-hidden">
                <div
                  className="h-full bg-neon transition-[width]"
                  style={{ width: pct == null ? "40%" : `${pct}%` }}
                />
              </div>
              <div className="text-[11px] text-white/50 mt-1">
                {pct == null ? "Descargando…" : `Descargando ${pct}%`} · se reiniciará al terminar
              </div>
            </div>
          ) : err ? (
            <div className="text-[12px] text-hot mt-0.5">{err}</div>
          ) : (
            update.body && (
              <div className="text-[12px] text-white/50 mt-0.5 line-clamp-2">{update.body}</div>
            )
          )}
        </div>
        {!busy && (
          <div className="flex items-center gap-2 shrink-0">
            <button
              onClick={() => setDismissed(true)}
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
