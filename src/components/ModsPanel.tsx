import { useEffect, useMemo, useState } from "react";
import { api, onModProgress } from "../api";
import type { ModInfo } from "../types";
import Icon from "../lib/icons";

interface Prog {
  pkg: string;
  index: number;
  total: number;
  pct: number;
  phase: string;
}

export default function ModsPanel({ projectId }: { projectId: string }) {
  const [mods, setMods] = useState<ModInfo[] | null>(null);
  // full_name -> installed version.
  const [installedVersions, setInstalledVersions] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [msg, setMsg] = useState<string | null>(null);
  // Progress keyed by the clicked mod's full_name → supports parallel installs.
  const [progress, setProgress] = useState<Record<string, Prog>>({});
  const [removing, setRemoving] = useState<Set<string>>(new Set());

  const refreshInstalled = () =>
    api.installedMods(projectId).then(setInstalledVersions).catch(() => {});

  useEffect(() => {
    api
      .listMods(projectId)
      .then(setMods)
      .catch((e) => setError(String(e)));
    refreshInstalled();
    const un = onModProgress((p) => {
      if (p.id !== projectId) return;
      setProgress((cur) => {
        if (!(p.target in cur)) return cur;
        return {
          ...cur,
          [p.target]: {
            pkg: p.pkg || cur[p.target].pkg,
            index: p.index,
            total: p.total,
            phase: p.phase,
            pct:
              p.phase === "download" && p.total_bytes > 0
                ? Math.round((p.downloaded / p.total_bytes) * 100)
                : p.phase === "extract"
                  ? 100
                  : cur[p.target].pct,
          },
        };
      });
    });
    return () => {
      un.then((f) => f());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId]);

  // Installed first, then by popularity; filter by search.
  const filtered = useMemo(() => {
    if (!mods) return [];
    const q = query.trim().toLowerCase();
    const list = q
      ? mods.filter((m) => `${m.name} ${m.owner} ${m.description}`.toLowerCase().includes(q))
      : mods;
    const sorted = [...list].sort((a, b) => {
      const ai = a.full_name in installedVersions ? 0 : 1;
      const bi = b.full_name in installedVersions ? 0 : 1;
      return ai - bi;
    });
    return sorted.slice(0, 80);
  }, [mods, query, installedVersions]);

  const flash = (m: string) => {
    setMsg(m);
    setTimeout(() => setMsg(null), 4000);
  };

  const install = (m: ModInfo) => {
    if (progress[m.full_name]) return;
    setProgress((cur) => ({
      ...cur,
      [m.full_name]: { pkg: m.name, index: 0, total: 0, pct: 0, phase: "start" },
    }));
    api
      .installMod(projectId, m.full_name)
      .then((files) => {
        refreshInstalled();
        flash(`Instalado: ${m.name} (${files.length} archivo${files.length === 1 ? "" : "s"}).`);
      })
      .catch((e) => flash(`${m.name}: ${String(e)}`))
      .finally(() =>
        setProgress((cur) => {
          const n = { ...cur };
          delete n[m.full_name];
          return n;
        }),
      );
  };

  const uninstall = (m: ModInfo) => {
    setRemoving((s) => new Set(s).add(m.full_name));
    api
      .uninstallMod(projectId, m.full_name)
      .then(() => refreshInstalled())
      .catch((e) => flash(String(e)))
      .finally(() =>
        setRemoving((s) => {
          const n = new Set(s);
          n.delete(m.full_name);
          return n;
        }),
      );
  };

  const activeCount = Object.keys(progress).length;

  return (
    <section>
      <div className="flex items-center gap-3 mb-3">
        <h2 className="text-lg font-black">Mods</h2>
        <span className="text-xs text-white/40">
          {mods ? `${mods.length} disponibles · ${Object.keys(installedVersions).length} instalados` : "…"}
          {activeCount > 0 && ` · descargando ${activeCount}`}
        </span>
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Buscar mod…"
          className="ml-auto rounded-md bg-panel-2 border border-edge px-3 py-1.5 text-sm outline-none focus:border-neon/50 w-48"
        />
      </div>

      {error && <div className="text-sm text-hot">{error}</div>}
      {msg && <div className="text-[13px] text-gold mb-2">{msg}</div>}

      {!mods && !error && <div className="text-white/40 text-sm">Cargando mods…</div>}

      <div className="grid gap-2 [grid-template-columns:repeat(auto-fill,minmax(260px,1fr))]">
        {filtered.map((m) => {
          const p = progress[m.full_name];
          const isInstalled = m.full_name in installedVersions;
          const hasUpdate = isInstalled && installedVersions[m.full_name] !== m.version;
          return (
            <div
              key={m.full_name}
              className={`flex gap-2.5 rounded-lg border bg-panel p-2.5 ${
                hasUpdate ? "border-gold/50" : isInstalled ? "border-neon-2/40" : "border-edge"
              }`}
            >
              {m.icon_url ? (
                <img
                  src={m.icon_url}
                  loading="lazy"
                  decoding="async"
                  className="w-12 h-12 rounded-md object-cover shrink-0 bg-panel-2"
                />
              ) : (
                <div className="w-12 h-12 rounded-md bg-panel-2 shrink-0" />
              )}
              <div className="min-w-0 flex-1">
                <div className="font-bold text-[13px] leading-tight truncate flex items-center gap-1.5" title={m.name}>
                  {m.name.replace(/_/g, " ")}
                  {hasUpdate ? (
                    <Icon.ArrowUp className="w-3 h-3 text-gold" />
                  ) : (
                    isInstalled && <Icon.Check className="w-3 h-3 text-neon-2" />
                  )}
                </div>
                <div className="text-[10px] text-white/40 truncate inline-flex items-center gap-1">
                  {m.owner} · <Icon.Download className="w-3 h-3" /> {m.downloads.toLocaleString()}
                </div>
                <p className="text-[11px] text-white/55 line-clamp-2 mt-0.5">{m.description}</p>
              </div>

              {p ? (
                <div className="self-center shrink-0 w-24">
                  <div className="h-1.5 rounded-full bg-edge overflow-hidden">
                    <div className="h-full bg-neon transition-[width]" style={{ width: `${p.pct}%` }} />
                  </div>
                  <div className="text-[9px] text-neon mt-1 truncate" title={p.pkg}>
                    {p.phase === "extract"
                      ? "Extrayendo…"
                      : p.total > 1
                        ? `${p.pct}% · ${p.index}/${p.total}`
                        : `${p.pct}%`}
                  </div>
                </div>
              ) : hasUpdate ? (
                <div className="self-center shrink-0 flex flex-col gap-1">
                  <button
                    onClick={() => install(m)}
                    className="rounded-md border border-gold/50 text-gold text-xs font-semibold px-2.5 py-1 hover:bg-gold/10"
                  >
                    Actualizar
                  </button>
                  <button
                    onClick={() => uninstall(m)}
                    disabled={removing.has(m.full_name)}
                    className="text-[10px] text-white/40 hover:text-hot disabled:opacity-50"
                  >
                    quitar
                  </button>
                </div>
              ) : isInstalled ? (
                <button
                  onClick={() => uninstall(m)}
                  disabled={removing.has(m.full_name)}
                  className="self-center shrink-0 rounded-md border border-hot/40 text-hot text-xs font-semibold px-2.5 py-1.5 hover:bg-hot/10 disabled:opacity-50"
                >
                  {removing.has(m.full_name) ? "…" : "Quitar"}
                </button>
              ) : (
                <button
                  onClick={() => install(m)}
                  className="self-center shrink-0 rounded-md border border-neon/40 text-neon text-xs font-semibold px-2.5 py-1.5 hover:bg-neon/10"
                >
                  Instalar
                </button>
              )}
            </div>
          );
        })}
      </div>
      {mods && filtered.length === 0 && <div className="text-white/40 text-sm">Sin resultados.</div>}
    </section>
  );
}
