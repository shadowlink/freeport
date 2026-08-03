import { useEffect, useMemo, useRef, useState } from "react";
import { api, onGamepad } from "../api";
import type { InstallProgress, ProjectView, SystemInfo, WikiInfo } from "../types";
import { gradientFor, screenshotsFrom } from "../lib/art";

interface Props {
  project: ProjectView;
  system?: SystemInfo;
  logoUrl?: string;
  progress?: InstallProgress;
  busy: boolean;
  disabled?: boolean;
  onPlay: () => void;
  onInstall: () => void;
  onChanged: () => void;
  onBack: () => void;
}

interface Action {
  label: string;
  run: () => void;
  kind: "primary" | "danger" | "normal";
}

export default function BigPictureDetail({
  project,
  system,
  logoUrl,
  progress,
  busy,
  disabled,
  onPlay,
  onInstall,
  onChanged,
  onBack,
}: Props) {
  const [wiki, setWiki] = useState<WikiInfo | null>(null);
  const [focus, setFocus] = useState(0);
  const shots = screenshotsFrom(project.cover_url);

  useEffect(() => {
    if (project.wiki) api.fetchWiki(project.wiki).then((w) => setWiki(w)).catch(() => {});
  }, [project.wiki]);

  const uninstall = () => {
    api
      .uninstallProject(project.id)
      .then(() => {
        onChanged();
        onBack();
      })
      .catch(() => {});
  };

  const actions = useMemo<Action[]>(() => {
    const a: Action[] = [];
    if (project.installed) {
      a.push({ label: "Jugar", run: onPlay, kind: "primary" });
      a.push({ label: "Eliminar", run: uninstall, kind: "danger" });
    } else {
      a.push({ label: "Instalar", run: onInstall, kind: "primary" });
    }
    a.push({ label: "Volver", run: onBack, kind: "normal" });
    return a;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [project.installed]);

  const handlerRef = useRef<(b: string) => void>(() => {});
  handlerRef.current = (b: string) => {
    if (disabled) return; // a game is running — ignore controller input
    if (b === "up" || b === "left") setFocus((v) => Math.max(0, v - 1));
    else if (b === "down" || b === "right") setFocus((v) => Math.min(actions.length - 1, v + 1));
    else if (b === "a") actions[Math.min(focus, actions.length - 1)]?.run();
  };
  useEffect(() => {
    const un = onGamepad((b) => handlerRef.current(b));
    return () => {
      un.then((f) => f());
    };
  }, []);

  const pct =
    progress && progress.phase === "download" && progress.total > 0
      ? Math.round((progress.downloaded / progress.total) * 100)
      : null;

  const heroBg = shots[0] ?? project.cover_url ?? undefined;

  return (
    <div className="fixed inset-0 z-40 bg-void text-white overflow-hidden">
      <div
        className="absolute inset-0 bg-cover bg-center blur-[4px] scale-110 opacity-60"
        style={heroBg ? { backgroundImage: `url(${heroBg})` } : { background: gradientFor(project.id) }}
      />
      <div className="absolute inset-0 bg-gradient-to-r from-void via-void/85 to-void/40" />

      <div className="relative h-full flex gap-8 p-12">
        {/* Left: info + actions */}
        <div className="w-[42%] flex flex-col">
          {logoUrl && <img src={logoUrl} alt="" className="h-8 mb-3 object-contain self-start" />}
          <h1 className="text-5xl font-black leading-none">{project.original_game || project.name}</h1>
          <div className="text-neon/90 font-semibold text-xl mt-1">{project.name}</div>
          <div className="text-white/60 mt-1">
            {[project.year, project.developer, project.genre, system?.name]
              .filter(Boolean)
              .join(" · ")}
          </div>

          {wiki && (
            <p className="mt-4 text-[15px] leading-relaxed text-white/75 line-clamp-5">
              {wiki.extract}
            </p>
          )}

          <div className="mt-auto space-y-3 pt-6 max-w-sm">
            {pct !== null ? (
              <div>
                <div className="h-2 rounded-full bg-edge overflow-hidden">
                  <div className="h-full bg-neon" style={{ width: `${pct}%` }} />
                </div>
                <div className="text-sm text-neon mt-1">Descargando {pct}%</div>
              </div>
            ) : busy ? (
              <div className="text-neon">
                <span className="spin inline-block w-4 h-4 border-2 border-neon border-t-transparent rounded-full mr-2" />
                Trabajando…
              </div>
            ) : (
              actions.map((act, i) => (
                <button
                  key={act.label}
                  onClick={act.run}
                  onMouseEnter={() => setFocus(i)}
                  className={`w-full text-left text-lg font-bold rounded-xl px-5 py-3 border-2 transition-all ${
                    i === focus ? "scale-[1.02]" : "opacity-80"
                  } ${
                    act.kind === "primary"
                      ? i === focus
                        ? "bg-neon text-void border-neon"
                        : "bg-neon/20 text-neon border-neon/40"
                      : act.kind === "danger"
                        ? i === focus
                          ? "bg-hot text-void border-hot"
                          : "bg-transparent text-hot border-hot/40"
                        : i === focus
                          ? "bg-white/15 border-white/60"
                          : "bg-transparent text-white/70 border-edge"
                  }`}
                >
                  {act.label}
                </button>
              ))
            )}

            {project.rom.required && !project.installed && (
              <p className="text-[13px] text-white/45">{project.rom.notes}</p>
            )}
            {project.rom.mode === "copy" && project.installed && !project.rom_configured && (
              <p className="text-[13px] text-gold">
                Este juego necesita que vincules su ROM (hazlo en modo escritorio).
              </p>
            )}
          </div>
        </div>

        {/* Right: box art + screenshots */}
        <div className="flex-1 flex flex-col items-center justify-center gap-4">
          <div
            className="w-56 aspect-[3/4] rounded-2xl bg-cover bg-center border border-white/10 shadow-2xl"
            style={
              project.cover_url
                ? { backgroundImage: `url(${project.cover_url})` }
                : { background: gradientFor(project.id) }
            }
          />
          {shots.length > 0 && (
            <div className="flex gap-3">
              {shots.map((sh) => (
                <img
                  key={sh}
                  src={sh}
                  loading="lazy"
                  className="h-28 rounded-lg border border-edge object-cover"
                  onError={(e) => ((e.target as HTMLImageElement).style.display = "none")}
                />
              ))}
            </div>
          )}
        </div>
      </div>

      <div className="absolute bottom-0 left-0 right-0 flex items-center gap-5 px-12 py-3 text-sm text-white/70 border-t border-edge bg-panel/60">
        <span>
          <b className="text-neon">Ⓐ</b> Aceptar
        </span>
        <span>
          <b className="text-neon-2">Ⓑ</b> Volver
        </span>
      </div>
    </div>
  );
}
