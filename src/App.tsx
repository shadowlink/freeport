import { useCallback, useEffect, useMemo, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { api, onInstallProgress } from "./api";
import { checkForUpdate, type Update } from "./lib/updater";
import type { CatalogView, InstallProgress, ProjectView, SystemInfo } from "./types";
import GameCard from "./components/GameCard";
import GamePage from "./components/GamePage";
import Settings from "./components/Settings";
import BigPicture from "./components/BigPicture";
import UpdateBanner from "./components/UpdateBanner";

export default function App() {
  const [catalog, setCatalog] = useState<CatalogView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<Record<string, InstallProgress>>({});
  const [busy, setBusy] = useState<Set<string>>(new Set());
  const [selected, setSelected] = useState<ProjectView | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  const [logos, setLogos] = useState<Record<string, string>>({});

  const [tab, setTab] = useState<"catalog" | "library">("catalog");
  const [activeSystem, setActiveSystem] = useState<string | null>(null); // null = todos
  const [query, setQuery] = useState("");
  const [tvMode, setTvMode] = useState(false);
  const [version, setVersion] = useState("");
  const [update, setUpdate] = useState<Update | null>(null);

  // Launched with `--tv` (e.g. by Sunshine) → open straight into Big Picture.
  useEffect(() => {
    api.isTvMode().then((tv) => tv && setTvMode(true)).catch(() => {});
    getVersion().then(setVersion).catch(() => {});
    checkForUpdate().then((u) => u && setUpdate(u));
  }, []);

  const checkAppUpdate = useCallback(async () => {
    const u = await checkForUpdate();
    if (u) setUpdate(u);
    return u;
  }, []);

  const load = useCallback(async () => {
    try {
      const view = await api.listCatalog();
      setCatalog(view);
      setError(null);
      const entries = await Promise.all(
        view.systems.map(async (s) => {
          const path = await api.systemLogo(s.id).catch(() => null);
          return [s.id, path ? convertFileSrc(path) : ""] as const;
        }),
      );
      setLogos(Object.fromEntries(entries.filter(([, v]) => v)));
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    load();
    // Best-effort: pull the latest catalog from the freeport-catalog repo, then
    // reload. Shows the cached/embedded catalog instantly; updates when the
    // remote fetch lands. Silently ignored when offline.
    api.refreshCatalog().then(() => load()).catch(() => {});
    const un = onInstallProgress((p) => {
      setProgress((prev) => ({ ...prev, [p.id]: p }));
      if (p.phase === "done") {
        setProgress((prev) => {
          const next = { ...prev };
          delete next[p.id];
          return next;
        });
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, [load]);

  const flash = (m: string) => {
    setToast(m);
    setTimeout(() => setToast(null), 3500);
  };

  const withBusy = async (id: string, fn: () => Promise<void>) => {
    setBusy((s) => new Set(s).add(id));
    try {
      await fn();
    } catch (e) {
      flash(String(e));
    } finally {
      setBusy((s) => {
        const n = new Set(s);
        n.delete(id);
        return n;
      });
    }
  };

  const install = (p: ProjectView) =>
    withBusy(p.id, async () => {
      await api.installProject(p.id);
      await load();
      setSelected((cur) => (cur && cur.id === p.id ? { ...cur, installed: true } : cur));
      flash(`${p.name} instalado.`);
    });

  const launch = (p: ProjectView) =>
    withBusy(p.id, async () => {
      if (p.is_windows) {
        flash(
          `Lanzando ${p.name} con Proton — la primera vez puede tardar 1–2 min (descarga el runtime y prepara el entorno).`,
        );
      }
      await api.launchProject(p.id);
      if (!p.is_windows) flash(`Lanzando ${p.name}…`);
    });

  const checkUpdates = async () => {
    setChecking(true);
    try {
      const updates = await api.checkUpdates();
      const withUpd = updates.filter((u) => u.update_available);
      if (catalog) {
        const ids = new Set(withUpd.map((u) => u.id));
        setCatalog({
          ...catalog,
          projects: catalog.projects.map((p) =>
            ids.has(p.id) ? { ...p, update_available: true } : p,
          ),
        });
      }
      flash(
        withUpd.length
          ? `Actualizaciones: ${withUpd.map((u) => u.name).join(", ")}`
          : "Todo está al día ✓",
      );
    } catch (e) {
      flash(String(e));
    } finally {
      setChecking(false);
    }
  };

  const systemsById = useMemo(() => {
    const m = new Map<string, SystemInfo>();
    catalog?.systems.forEach((s) => m.set(s.id, s));
    return m;
  }, [catalog]);

  // Projects for the current tab (catalog/library), before system/search filters.
  const tabProjects = useMemo(() => {
    if (!catalog) return [];
    return tab === "library" ? catalog.projects.filter((p) => p.installed) : catalog.projects;
  }, [catalog, tab]);

  // Sidebar: systems present in the current tab + their counts.
  const sidebarSystems = useMemo(() => {
    const counts = new Map<string, number>();
    for (const p of tabProjects) counts.set(p.system, (counts.get(p.system) ?? 0) + 1);
    const order = catalog?.systems.map((s) => s.id) ?? [];
    const ids = [
      ...order.filter((id) => counts.has(id)),
      ...[...counts.keys()].filter((id) => !order.includes(id)),
    ];
    return ids.map((id) => ({
      system: systemsById.get(id),
      id,
      count: counts.get(id)!,
    }));
  }, [tabProjects, catalog, systemsById]);

  // Apply system + search filters, then group by console for display.
  const grouped = useMemo(() => {
    const q = query.trim().toLowerCase();
    const filtered = tabProjects.filter((p) => {
      if (activeSystem && p.system !== activeSystem) return false;
      if (q && !`${p.original_game} ${p.name}`.toLowerCase().includes(q)) return false;
      return true;
    });
    const map = new Map<string, ProjectView[]>();
    for (const p of filtered) {
      if (!map.has(p.system)) map.set(p.system, []);
      map.get(p.system)!.push(p);
    }
    const order = catalog?.systems.map((s) => s.id) ?? [];
    const ids = [
      ...order.filter((id) => map.has(id)),
      ...[...map.keys()].filter((id) => !order.includes(id)),
    ];
    return ids.map((id) => ({ system: systemsById.get(id), id, projects: map.get(id)! }));
  }, [tabProjects, activeSystem, query, catalog, systemsById]);

  const installedCount = catalog?.projects.filter((p) => p.installed).length ?? 0;
  const total = grouped.reduce((n, g) => n + g.projects.length, 0);

  const NavRow = ({
    active,
    onClick,
    logo,
    color,
    label,
    count,
  }: {
    active: boolean;
    onClick: () => void;
    logo?: string;
    color?: string;
    label: string;
    count: number;
  }) => (
    <button
      onClick={onClick}
      title={label}
      className={`nav-item w-full flex items-center gap-2.5 px-2.5 py-2.5 rounded-lg text-left ${
        active ? "bg-neon/15 text-white" : "text-white/60 hover:bg-white/5 hover:text-white/90"
      }`}
    >
      <span
        className="w-1 h-6 rounded-full shrink-0"
        style={{ background: active ? color ?? "var(--color-neon)" : "transparent" }}
      />
      {logo ? (
        <img
          src={logo}
          alt={label}
          className="h-7 w-auto max-w-[150px] object-contain object-left flex-1 min-w-0 opacity-90"
        />
      ) : (
        <>
          <span
            className="w-2.5 h-2.5 rounded-full shrink-0"
            style={{ background: color ?? "#888" }}
          />
          <span className="text-sm font-semibold truncate flex-1">{label}</span>
        </>
      )}
      <span className="text-[11px] text-white/35 shrink-0">{count}</span>
    </button>
  );

  return (
    <div className="h-full flex text-white">
      <UpdateBanner update={update} onDismiss={() => setUpdate(null)} />
      {/* ── Sidebar ─────────────────────────────────────────── */}
      <aside className="w-60 shrink-0 border-r border-edge bg-panel/60 flex flex-col">
        <div className="flex items-center gap-2 px-4 py-4">
          <div className="w-8 h-8 rounded-lg grid place-items-center bg-neon text-void font-black">
            ⚓
          </div>
          <div className="leading-none">
            <div className="font-black tracking-wide">
              FREE<span className="text-neon">PORT</span>
            </div>
            <div className="text-[10px] text-white/35 mt-0.5">
              {version ? `v${version}` : ""}
              {catalog ? ` · ${catalog.platform}` : ""}
            </div>
          </div>
        </div>

        <div className="px-3 grid grid-cols-2 gap-1.5">
          {(["catalog", "library"] as const).map((t) => (
            <button
              key={t}
              onClick={() => {
                setTab(t);
                setActiveSystem(null);
              }}
              className={`py-1.5 rounded-lg text-sm font-bold transition-colors ${
                tab === t ? "bg-neon text-void" : "bg-panel-2 text-white/60 hover:text-white"
              }`}
            >
              {t === "catalog" ? "Catálogo" : `Biblioteca`}
            </button>
          ))}
        </div>

        <div className="px-3 mt-3">
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Buscar juego…"
            className="w-full rounded-lg bg-panel-2 border border-edge px-3 py-2 text-sm outline-none focus:border-neon/50 placeholder:text-white/30"
          />
        </div>

        <nav className="flex-1 overflow-y-auto thin-scroll px-2 mt-3 pb-2 space-y-0.5">
          <NavRow
            active={activeSystem === null}
            onClick={() => setActiveSystem(null)}
            label="Todos los juegos"
            count={tabProjects.length}
          />
          {sidebarSystems.map(({ id, system, count }) => (
            <NavRow
              key={id}
              active={activeSystem === id}
              onClick={() => setActiveSystem(id)}
              logo={logos[id]}
              color={system?.color}
              label={system?.name ?? id.toUpperCase()}
              count={count}
            />
          ))}
        </nav>

        <div className="p-3 border-t border-edge space-y-2">
          <button
            onClick={() => setTvMode(true)}
            className="w-full rounded-lg bg-neon/15 border border-neon/40 text-neon px-2 py-2 text-sm font-bold hover:bg-neon/25"
          >
            📺 Modo TV
          </button>
          <div className="flex gap-2">
            <button
              onClick={checkUpdates}
              disabled={checking}
              className="flex-1 rounded-lg border border-edge px-2 py-2 text-xs font-semibold hover:border-neon/50 disabled:opacity-50"
            >
              {checking ? "Buscando…" : "Buscar updates"}
            </button>
            <button
              onClick={() => setShowSettings(true)}
              className="rounded-lg border border-edge px-3 py-2 text-xs font-semibold hover:border-neon/50"
              title="Ajustes"
            >
              ⚙
            </button>
          </div>
        </div>
      </aside>

      {/* ── Main ────────────────────────────────────────────── */}
      <main className="flex-1 flex flex-col min-w-0">
        <header className="flex items-baseline gap-3 px-6 py-4 border-b border-edge">
          <h1 className="text-2xl font-black tracking-wide">
            {activeSystem
              ? systemsById.get(activeSystem)?.name ?? activeSystem.toUpperCase()
              : tab === "library"
                ? "Mi biblioteca"
                : "Catálogo"}
          </h1>
          <span className="text-sm text-white/40">
            {total} {total === 1 ? "juego" : "juegos"}
            {tab === "catalog" && ` · ${installedCount} instalados`}
          </span>
        </header>

        <div className="flex-1 overflow-y-auto thin-scroll px-6 py-5">
          {error && (
            <div className="mb-4 rounded-lg border border-hot/50 bg-hot/10 text-hot px-4 py-3 text-sm">
              {error}
            </div>
          )}
          {!catalog && !error && <div className="text-white/50">Cargando catálogo…</div>}

          {catalog && total === 0 && (
            <div className="mt-16 text-center text-white/50">
              {query ? (
                <>Sin resultados para «{query}».</>
              ) : tab === "library" ? (
                <>
                  <div className="text-lg font-bold text-white/70">
                    Aún no tienes juegos instalados
                  </div>
                  <p className="mt-1 text-sm">
                    Ve al{" "}
                    <button
                      onClick={() => setTab("catalog")}
                      className="text-neon underline underline-offset-2"
                    >
                      Catálogo
                    </button>{" "}
                    para instalar tu primer juego.
                  </p>
                </>
              ) : (
                <>No hay juegos lanzables aquí.</>
              )}
            </div>
          )}

          {grouped.map(({ system, id, projects }) => (
            <section key={id} className="cv-section mb-8">
              {/* Console header only when showing "Todos" (grouped view). */}
              {activeSystem === null && (
                <div className="flex items-center gap-3 mb-3">
                  {logos[id] ? (
                    <img
                      src={logos[id]}
                      alt={system?.name ?? id}
                      className="h-7 w-auto max-w-[200px] object-contain"
                    />
                  ) : (
                    <h2 className="text-lg font-black tracking-wide">
                      {system?.name ?? id.toUpperCase()}
                    </h2>
                  )}
                  <span className="text-xs text-white/35">{projects.length}</span>
                </div>
              )}
              <div className="grid gap-4 [grid-template-columns:repeat(auto-fill,minmax(168px,1fr))]">
                {projects.map((p) => (
                  <GameCard
                    key={p.id}
                    project={p}
                    system={system}
                    progress={progress[p.id]}
                    busy={busy.has(p.id)}
                    onInstall={() => install(p)}
                    onLaunch={() => launch(p)}
                    onDetails={() => setSelected(p)}
                  />
                ))}
              </div>
            </section>
          ))}
        </div>
      </main>

      {toast && (
        <div className="fixed bottom-5 left-1/2 -translate-x-1/2 z-50 max-w-[90vw] rounded-lg border border-neon/40 bg-panel px-4 py-2.5 text-sm">
          {toast}
        </div>
      )}

      {selected && (
        <GamePage
          project={catalog?.projects.find((p) => p.id === selected.id) ?? selected}
          system={systemsById.get(selected.system)}
          logoUrl={logos[selected.system]}
          onClose={() => setSelected(null)}
          onChanged={load}
          onLaunch={() => launch(selected)}
          onInstall={() => install(selected)}
        />
      )}
      {showSettings && (
        <Settings
          onClose={() => setShowSettings(false)}
          onChanged={load}
          version={version}
          onCheckUpdate={checkAppUpdate}
        />
      )}

      {tvMode && catalog && (
        <BigPicture
          catalog={catalog}
          logos={logos}
          progress={progress}
          busy={busy}
          onInstall={install}
          onChanged={load}
          onExit={() => setTvMode(false)}
        />
      )}
    </div>
  );
}
