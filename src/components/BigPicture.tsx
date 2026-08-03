import { useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, onGamepad, onGameExited } from "../api";
import type { CatalogView, InstallProgress, ProjectView, SystemInfo } from "../types";
import { gradientFor, initials, screenshotsFrom } from "../lib/art";
import BigPictureDetail from "./BigPictureDetail";

interface Props {
  catalog: CatalogView;
  logos: Record<string, string>;
  progress: Record<string, InstallProgress>;
  busy: Set<string>;
  onInstall: (p: ProjectView) => void;
  onChanged: () => void;
  onExit: () => void;
}

interface Shelf {
  system?: SystemInfo;
  id: string;
  projects: ProjectView[];
}

export default function BigPicture({
  catalog,
  logos,
  progress,
  busy,
  onInstall,
  onChanged,
  onExit,
}: Props) {
  const systemsById = useMemo(() => {
    const m = new Map<string, SystemInfo>();
    catalog.systems.forEach((s) => m.set(s.id, s));
    return m;
  }, [catalog]);

  const shelves = useMemo<Shelf[]>(() => {
    const map = new Map<string, ProjectView[]>();
    for (const p of catalog.projects) {
      if (!map.has(p.system)) map.set(p.system, []);
      map.get(p.system)!.push(p);
    }
    const order = [
      ...catalog.systems.map((s) => s.id).filter((id) => map.has(id)),
      ...[...map.keys()].filter((id) => !catalog.systems.some((s) => s.id === id)),
    ];
    return order.map((id) => ({ system: systemsById.get(id), id, projects: map.get(id)! }));
  }, [catalog, systemsById]);

  const [s, setS] = useState(0);
  const [c, setC] = useState(0);
  const [view, setView] = useState<"grid" | "detail">("grid");
  const [detail, setDetail] = useState<ProjectView | null>(null);
  const [playing, setPlaying] = useState<ProjectView | null>(null);
  const [toast, setToast] = useState<string | null>(null);

  const focused: ProjectView | undefined = shelves[s]?.projects[c];

  // Keep a live handler for the stable input listener (avoids stale closures).
  const handlerRef = useRef<(b: string) => void>(() => {});

  const flash = (m: string) => {
    setToast(m);
    setTimeout(() => setToast(null), 3500);
  };

  const reassertFullscreen = async () => {
    try {
      const w = getCurrentWindow();
      await w.unminimize();
      await w.setFullscreen(true);
      await w.setFocus();
      await w.setCursorVisible(false);
    } catch {
      /* ignore */
    }
  };

  const play = (p: ProjectView) => {
    setPlaying(p);
    api
      .launchProject(p.id)
      .then(async () => {
        // Get the launcher out of the way so the game window takes the screen
        // and the controller input goes to the game, not to us.
        try {
          const w = getCurrentWindow();
          await w.setFullscreen(false);
          await w.minimize();
        } catch {
          /* ignore */
        }
      })
      .catch((e) => {
        setPlaying(null);
        flash(String(e));
      });
  };

  // Enter fullscreen on mount; restore on exit.
  useEffect(() => {
    reassertFullscreen();
    const w = getCurrentWindow();
    return () => {
      w.setFullscreen(false).catch(() => {});
      w.setCursorVisible(true).catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Return from a game → drop overlay + re-take fullscreen focus.
  useEffect(() => {
    const un = onGameExited(() => {
      setPlaying(null);
      reassertFullscreen();
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  // Scroll focused card into view when focus changes (grid).
  useEffect(() => {
    if (view !== "grid") return;
    const el = document.getElementById(`bp-${s}-${c}`);
    el?.scrollIntoView({ behavior: "smooth", inline: "center", block: "center" });
  }, [s, c, view]);

  // ── Input handling (gamepad + keyboard) ──────────────────────────────
  handlerRef.current = (b: string) => {
    if (playing) return; // ignore input while a game runs
    if (view === "detail") {
      // Detail handles its own focus; here we only catch Back.
      if (b === "b") {
        setView("grid");
      }
      // a/up/down handled by BigPictureDetail via its own listener.
      return;
    }
    const shelf = shelves[s];
    switch (b) {
      case "up":
        setS((v) => {
          const ns = Math.max(0, v - 1);
          setC((cc) => Math.min(cc, (shelves[ns]?.projects.length ?? 1) - 1));
          return ns;
        });
        break;
      case "down":
        setS((v) => {
          const ns = Math.min(shelves.length - 1, v + 1);
          setC((cc) => Math.min(cc, (shelves[ns]?.projects.length ?? 1) - 1));
          return ns;
        });
        break;
      case "left":
        setC((v) => Math.max(0, v - 1));
        break;
      case "right":
        setC((v) => Math.min((shelf?.projects.length ?? 1) - 1, v + 1));
        break;
      case "lb":
        setS((v) => {
          const ns = Math.max(0, v - 1);
          setC(0);
          return ns;
        });
        break;
      case "rb":
        setS((v) => {
          const ns = Math.min(shelves.length - 1, v + 1);
          setC(0);
          return ns;
        });
        break;
      case "a":
        if (shelf?.projects[c]) {
          setDetail(shelf.projects[c]);
          setView("detail");
        }
        break;
      case "start":
        onExit();
        break;
    }
  };

  useEffect(() => {
    const un = onGamepad((b) => handlerRef.current(b));
    const keymap: Record<string, string> = {
      ArrowUp: "up",
      ArrowDown: "down",
      ArrowLeft: "left",
      ArrowRight: "right",
      Enter: "a",
      Backspace: "b",
      Escape: "start",
      PageUp: "lb",
      PageDown: "rb",
    };
    const onKey = (e: KeyboardEvent) => {
      const m = keymap[e.key];
      if (m) {
        e.preventDefault();
        handlerRef.current(m);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      un.then((f) => f());
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  const heroBg = focused
    ? screenshotsFrom(focused.cover_url)[0] ?? focused.cover_url ?? undefined
    : undefined;

  return (
    <div className="fixed inset-0 z-30 bg-void text-white flex flex-col overflow-hidden select-none">
      {/* Hero */}
      <div className="relative h-[42vh] shrink-0">
        <div
          className="absolute inset-0 bg-cover bg-center blur-[3px] scale-105 transition-[background-image] duration-300"
          style={
            heroBg
              ? { backgroundImage: `url(${heroBg})` }
              : { background: focused ? gradientFor(focused.id) : "#0b0c10" }
          }
        />
        <div className="absolute inset-0 bg-gradient-to-t from-void via-void/60 to-void/20" />
        {focused && (
          <div className="absolute bottom-6 left-10 right-10 flex items-end gap-6 z-10">
            <div
              className="w-40 shrink-0 aspect-[3/4] rounded-xl border border-white/10 bg-cover bg-center shadow-2xl"
              style={
                focused.cover_url
                  ? { backgroundImage: `url(${focused.cover_url})` }
                  : { background: gradientFor(focused.id) }
              }
            />
            <div className="min-w-0 pb-1">
              {logos[focused.system] && (
                <img src={logos[focused.system]} alt="" className="h-8 mb-2 object-contain" />
              )}
              <h1 className="text-4xl font-black leading-tight drop-shadow">
                {focused.original_game || focused.name}
              </h1>
              <div className="text-neon/90 font-semibold text-lg">{focused.name}</div>
              <div className="mt-1 text-white/60">
                {[focused.year, focused.developer, focused.genre].filter(Boolean).join(" · ")}
              </div>
              <div className="mt-2 flex gap-2 text-sm">
                {focused.installed ? (
                  <span className="px-2 py-0.5 rounded bg-neon-2/20 text-neon-2 border border-neon-2/40">
                    Instalado
                  </span>
                ) : (
                  <span className="px-2 py-0.5 rounded bg-neon/15 text-neon border border-neon/40">
                    Sin instalar
                  </span>
                )}
                {focused.is_windows && (
                  <span className="px-2 py-0.5 rounded bg-[#2b6fb3]/30 text-[#7fb2e0] border border-[#2b6fb3]/50">
                    Windows
                  </span>
                )}
              </div>
            </div>
          </div>
        )}
      </div>

      {/* Shelves */}
      <div className="flex-1 overflow-y-auto thin-scroll px-10 py-4 space-y-6">
        {shelves.map((shelf, si) => (
          <section key={shelf.id}>
            <div className="flex items-center gap-3 mb-2">
              {logos[shelf.id] ? (
                <img src={logos[shelf.id]} alt="" className="h-7 object-contain" />
              ) : (
                <h2 className="text-xl font-black">{shelf.system?.name ?? shelf.id}</h2>
              )}
              <span className="text-xs text-white/35">{shelf.projects.length}</span>
            </div>
            <div className="flex gap-4 overflow-x-auto thin-scroll pb-2">
              {shelf.projects.map((p, ci) => {
                const isFocus = si === s && ci === c && view === "grid";
                return (
                  <div
                    id={`bp-${si}-${ci}`}
                    key={p.id}
                    onClick={() => {
                      setS(si);
                      setC(ci);
                      setDetail(p);
                      setView("detail");
                    }}
                    className={`w-[150px] shrink-0 rounded-xl overflow-hidden border-2 transition-transform cursor-pointer ${
                      isFocus
                        ? "border-neon scale-105 shadow-[0_0_24px_rgba(255,178,62,0.4)]"
                        : "border-transparent opacity-80"
                    }`}
                  >
                    <div
                      className="aspect-[3/4] bg-cover bg-center bg-panel-2 grid place-items-center"
                      style={
                        p.cover_url
                          ? { backgroundImage: `url(${p.cover_url})` }
                          : { background: gradientFor(p.id) }
                      }
                    >
                      {!p.cover_url && (
                        <span className="text-3xl font-black text-white/80">
                          {initials(p.original_game || p.name)}
                        </span>
                      )}
                    </div>
                    {p.installed && <div className="h-1 bg-neon-2" />}
                  </div>
                );
              })}
            </div>
          </section>
        ))}
      </div>

      {/* Hint bar */}
      <div className="shrink-0 flex items-center gap-5 px-10 py-3 border-t border-edge bg-panel/70 text-sm text-white/70">
        <span>
          <b className="text-neon">Ⓐ</b> Seleccionar
        </span>
        <span>
          <b className="text-neon-2">Ⓑ</b> Volver
        </span>
        <span>
          <b className="text-white/80">LB/RB</b> Consola
        </span>
        <span className="ml-auto">
          <b className="text-white/80">☰ Start</b> Salir del modo TV
        </span>
      </div>

      {view === "detail" && detail && (
        <BigPictureDetail
          project={catalog.projects.find((p) => p.id === detail.id) ?? detail}
          system={systemsById.get(detail.system)}
          logoUrl={logos[detail.system]}
          progress={progress[detail.id]}
          busy={busy.has(detail.id)}
          disabled={!!playing}
          onPlay={() => play(detail)}
          onInstall={() => onInstall(detail)}
          onChanged={onChanged}
          onBack={() => setView("grid")}
        />
      )}

      {playing && (
        <div className="fixed inset-0 z-50 bg-void grid place-items-center text-center">
          <div>
            <div className="text-2xl font-black mb-2">Jugando a {playing.name}</div>
            <div className="text-white/50">
              Cierra el juego para volver a Freeport.
              {playing.is_windows && " (La primera vez con Proton puede tardar 1–2 min.)"}
            </div>
            <div className="mt-4">
              <span className="spin inline-block w-6 h-6 border-2 border-neon border-t-transparent rounded-full" />
            </div>
          </div>
        </div>
      )}

      {toast && (
        <div className="fixed bottom-20 left-1/2 -translate-x-1/2 z-50 rounded-lg border border-hot/50 bg-panel px-4 py-2.5 text-sm max-w-[80vw]">
          {toast}
        </div>
      )}
    </div>
  );
}
