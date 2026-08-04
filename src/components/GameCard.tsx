import { useState } from "react";
import type { ProjectView, SystemInfo, InstallProgress } from "../types";
import { gradientFor, initials, thumbUrl } from "../lib/art";
import Icon from "../lib/icons";

interface Props {
  project: ProjectView;
  system?: SystemInfo;
  platform: string;
  progress?: InstallProgress;
  busy: boolean;
  onInstall: () => void;
  onLaunch: () => void;
  onDetails: () => void;
}

export default function GameCard({
  project,
  system,
  platform,
  progress,
  busy,
  onInstall,
  onLaunch,
  onDetails,
}: Props) {
  // Try the cached thumbnail first, then the full cover, then a gradient.
  const sources = [thumbUrl(project.cover_url, platform), project.cover_url].filter(
    Boolean,
  ) as string[];
  const [srcIdx, setSrcIdx] = useState(0);
  const artSrc = sources[srcIdx] ?? null;
  const showArt = artSrc != null;

  const pct =
    progress && progress.phase === "download" && progress.total > 0
      ? Math.round((progress.downloaded / progress.total) * 100)
      : null;
  const working = pct !== null || (progress && progress.phase === "extract");

  return (
    <div className="card rounded-xl border border-edge bg-panel overflow-hidden flex flex-col">
      <button
        onClick={onDetails}
        className="relative block w-full aspect-[3/4] overflow-hidden bg-panel-2 text-left"
        title={project.original_game}
      >
        {showArt ? (
          <img
            src={artSrc!}
            onError={() => setSrcIdx((i) => i + 1)}
            alt={project.original_game}
            loading="lazy"
            decoding="async"
            className="w-full h-full object-cover"
          />
        ) : (
          <div
            className="w-full h-full grid place-items-center poster"
            style={{ background: gradientFor(project.id) }}
          >
            <span className="text-4xl font-black text-white/85">
              {initials(project.original_game || project.name)}
            </span>
          </div>
        )}

        {/* status corner marks */}
        <span className="absolute top-2 left-2 flex items-center gap-1.5">
          <span
            className={`text-[9px] font-bold tracking-widest px-1.5 py-0.5 rounded bg-black/55 border ${
              project.type === "recompilation"
                ? "text-neon border-neon/40"
                : "text-neon-2 border-neon-2/40"
            }`}
          >
            {project.type === "recompilation" ? "RECOMP" : "PORT"}
          </span>
          {project.is_windows && (
            <span
              className="text-[9px] font-bold tracking-widest px-1.5 py-0.5 rounded bg-[#2b6fb3] text-white inline-flex items-center gap-1"
              title="Ejecutable de Windows — se ejecuta con Wine/Proton"
            >
              <Icon.Windows className="w-2.5 h-2.5" /> WIN
            </span>
          )}
        </span>
        {project.installed && (
          <span
            className="absolute top-2 right-2 w-2.5 h-2.5 rounded-full bg-neon-2 ring-2 ring-black/40"
            title="Instalado"
          />
        )}
        {project.update_available && (
          <span className="badge-pulse absolute bottom-2 left-2 text-[9px] font-bold tracking-widest px-1.5 py-0.5 rounded bg-black/60 text-gold border border-gold/50">
            UPDATE
          </span>
        )}
        {system && (
          <span
            className="absolute bottom-2 right-2 text-[9px] font-bold tracking-widest px-1.5 py-0.5 rounded"
            style={{ background: system.color + "e6", color: "#0b0c10" }}
          >
            {system.short}
          </span>
        )}
      </button>

      <div className="p-2.5 flex flex-col flex-1">
        <div
          className="font-bold text-[13px] leading-tight line-clamp-2 min-h-[2.3em]"
          title={project.original_game}
        >
          {project.original_game || project.name}
        </div>
        <div className="text-[10px] text-white/40 truncate mb-2" title={project.name}>
          {project.name}
        </div>

        <div className="mt-auto">
          {pct !== null ? (
            <div>
              <div className="h-1.5 rounded-full bg-edge overflow-hidden">
                <div className="h-full bg-neon" style={{ width: `${pct}%` }} />
              </div>
              <div className="text-[10px] text-neon mt-1">Descargando {pct}%</div>
            </div>
          ) : working ? (
            <div className="h-8 flex items-center text-[11px] text-neon">
              <span className="spin inline-block w-3 h-3 border-2 border-neon border-t-transparent rounded-full mr-2" />
              Extrayendo…
            </div>
          ) : project.installed ? (
            <button
              onClick={onLaunch}
              disabled={busy}
              className="w-full h-8 rounded-md bg-neon text-void font-bold text-sm hover:brightness-110 disabled:opacity-50 inline-flex items-center justify-center gap-1.5"
            >
              <Icon.Play className="w-3.5 h-3.5" /> Jugar
            </button>
          ) : (
            <button
              onClick={onInstall}
              disabled={busy}
              className="w-full h-8 rounded-md border border-neon/40 text-neon font-semibold text-sm hover:bg-neon/10 disabled:opacity-50 inline-flex items-center justify-center gap-1.5"
            >
              <Icon.Download className="w-3.5 h-3.5" /> Instalar
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
