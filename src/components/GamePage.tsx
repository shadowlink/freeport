import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { ProjectView, Runner, SystemInfo, WikiInfo } from "../types";
import { api } from "../api";
import { gradientFor, screenshotsFrom } from "../lib/art";
import ModsPanel from "./ModsPanel";

interface Props {
  project: ProjectView;
  system?: SystemInfo;
  logoUrl?: string;
  onClose: () => void;
  onChanged: () => void;
  onLaunch: () => void;
  onInstall: () => void;
}

export default function GamePage({
  project,
  system,
  logoUrl,
  onClose,
  onChanged,
  onLaunch,
  onInstall,
}: Props) {
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [wiki, setWiki] = useState<WikiInfo | null>(null);
  const [wikiLoading, setWikiLoading] = useState(false);
  const [failed, setFailed] = useState<Set<string>>(new Set());
  const [lightbox, setLightbox] = useState<string | null>(null);
  const [runners, setRunners] = useState<Runner[]>([]);
  const [gameRunner, setGameRunner] = useState<string>(""); // "" = default

  const romInApp = project.rom.mode === "in-app";
  const romNone = project.rom.mode === "none";
  const shots = screenshotsFrom(project.cover_url).filter((s) => !failed.has(s));
  const heroBg = shots[0] ?? project.cover_url ?? undefined;

  useEffect(() => {
    let active = true;
    setWiki(null);
    if (project.wiki) {
      setWikiLoading(true);
      api
        .fetchWiki(project.wiki)
        .then((w) => active && setWiki(w))
        .catch(() => {})
        .finally(() => active && setWikiLoading(false));
    }
    return () => {
      active = false;
    };
  }, [project.wiki]);

  useEffect(() => {
    if (!project.is_windows) return;
    api.listRunners().then(setRunners).catch(() => {});
    api.getConfig().then((c) => setGameRunner(c.game_runners?.[project.id] ?? ""));
  }, [project.is_windows, project.id]);

  async function changeGameRunner(v: string) {
    setGameRunner(v);
    await api.setGameRunner(project.id, v || null);
  }

  async function pickRom() {
    const file = await open({ multiple: false, title: `ROM de ${project.original_game}` });
    if (typeof file !== "string") return;
    setBusy(true);
    setMsg(null);
    try {
      await api.setRom(project.id, file);
      setMsg("ROM vinculada correctamente.");
      onChanged();
    } catch (e) {
      setMsg(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function uninstall() {
    setBusy(true);
    try {
      await api.uninstallProject(project.id);
      onChanged();
      onClose();
    } catch (e) {
      setMsg(String(e));
    } finally {
      setBusy(false);
    }
  }

  const repoUrl = `https://github.com/${project.repo.owner}/${project.repo.repo}`;
  const facts: [string, string][] = [];
  if (project.year) facts.push(["Año", String(project.year)]);
  if (project.developer) facts.push(["Desarrollador", project.developer]);
  if (project.genre) facts.push(["Género", project.genre]);
  facts.push(["Sistema", system?.name ?? project.system.toUpperCase()]);
  facts.push(["Tipo", project.type === "recompilation" ? "Recompilación" : "Port nativo"]);
  if (project.installed_tag) facts.push(["Versión", project.installed_tag]);

  return (
    <div className="fixed inset-0 z-40 bg-void overflow-y-auto thin-scroll">
      {/* Hero */}
      <div className="relative h-72">
        <div
          className="absolute inset-0 bg-cover bg-center scale-105 blur-[2px]"
          style={heroBg ? { backgroundImage: `url(${heroBg})` } : { background: gradientFor(project.id) }}
        />
        <div className="absolute inset-0 bg-gradient-to-t from-void via-void/70 to-void/30" />

        <button
          onClick={onClose}
          className="absolute top-4 left-4 z-10 rounded-lg bg-black/50 border border-white/15 px-3 py-1.5 text-sm hover:bg-black/70"
        >
          ← Volver
        </button>

        <div className="absolute bottom-0 left-0 right-0 p-6 flex items-end gap-5">
          <div
            className="w-32 shrink-0 aspect-[3/4] rounded-lg border border-white/10 bg-panel-2 poster shadow-xl overflow-hidden"
            style={
              project.cover_url
                ? { backgroundImage: `url(${project.cover_url})` }
                : { background: gradientFor(project.id) }
            }
          />
          <div className="min-w-0">
            {logoUrl && <img src={logoUrl} alt="" className="h-6 mb-2 object-contain" />}
            <h1 className="text-3xl font-black leading-tight drop-shadow">{project.original_game}</h1>
            <div className="text-neon/90 font-semibold">{project.name}</div>
            <div className="mt-1 text-sm text-white/60 flex flex-wrap gap-x-2">
              {[project.year, project.developer, project.genre].filter(Boolean).join(" · ")}
            </div>
          </div>
        </div>
      </div>

      {/* Body */}
      <div className="max-w-5xl mx-auto px-6 py-6 grid grid-cols-1 lg:grid-cols-[1fr_300px] gap-6">
        {/* Main column */}
        <div className="space-y-6 min-w-0">
          {project.is_windows && (
            <div className="rounded-lg border border-[#2b6fb3]/60 bg-[#2b6fb3]/15 px-4 py-3 text-sm flex items-start gap-2">
              <span className="text-[#7fb2e0] font-bold shrink-0">⊞ WINDOWS</span>
              <span className="text-white/80">
                Este juego solo tiene <b>ejecutable de Windows</b>. Freeport lo
                lanzará con <b>Wine/Proton</b> (necesitas tenerlo instalado). La
                compatibilidad no está garantizada.
              </span>
            </div>
          )}
          {shots.length > 0 && (
            <div className="flex gap-3 overflow-x-auto thin-scroll pb-1">
              {shots.map((s) => (
                <img
                  key={s}
                  src={s}
                  loading="lazy"
                  decoding="async"
                  onError={() => setFailed((f) => new Set(f).add(s))}
                  onClick={() => setLightbox(s)}
                  className="h-44 rounded-lg border border-edge object-cover cursor-zoom-in shrink-0"
                />
              ))}
            </div>
          )}

          <section>
            <h2 className="text-lg font-black mb-2">Acerca del juego</h2>
            {wikiLoading && <div className="text-white/40 text-sm">Cargando…</div>}
            {wiki ? (
              <>
                <p className="text-[15px] leading-relaxed text-white/80 whitespace-pre-line">
                  {wiki.extract}
                </p>
                <div className="mt-2 text-[11px] text-white/35">
                  Texto de{" "}
                  <button
                    onClick={() => wiki.url && openUrl(wiki.url)}
                    className="underline hover:text-white/60"
                  >
                    Wikipedia
                  </button>{" "}
                  (CC BY-SA).
                </div>
              </>
            ) : (
              !wikiLoading && (
                <p className="text-[15px] leading-relaxed text-white/60">
                  {project.rom.notes || "Sin descripción disponible."}
                </p>
              )
            )}
          </section>

          <section>
            <h2 className="text-lg font-black mb-2">Sobre este port</h2>
            <p className="text-[14px] leading-relaxed text-white/70">{project.rom.notes}</p>
          </section>

          {project.mods && <ModsPanel projectId={project.id} />}
        </div>

        {/* Sidebar */}
        <aside className="space-y-4">
          <div className="rounded-xl border border-edge bg-panel p-4">
            {project.installed ? (
              <div className="space-y-2">
                <button
                  onClick={onLaunch}
                  disabled={busy}
                  className="w-full rounded-lg bg-neon text-void font-bold py-2.5 hover:brightness-110 disabled:opacity-50"
                >
                  ▶ Jugar
                </button>
                {project.update_available && (
                  <button
                    onClick={onInstall}
                    className="w-full rounded-lg border border-gold/50 text-gold py-2 hover:bg-gold/10"
                  >
                    Actualizar
                  </button>
                )}
                <button
                  onClick={uninstall}
                  disabled={busy}
                  className="w-full rounded-lg border border-hot/50 text-hot py-2 hover:bg-hot/10 disabled:opacity-50"
                >
                  Eliminar
                </button>
              </div>
            ) : (
              <button
                onClick={onInstall}
                disabled={busy}
                className="w-full rounded-lg bg-neon text-void font-bold py-2.5 hover:brightness-110 disabled:opacity-50"
              >
                ⭳ Instalar
              </button>
            )}

            {/* Windows runner selector */}
            {project.is_windows && (
              <div className="mt-3 pt-3 border-t border-edge text-[13px]">
                <div className="font-bold text-[#7fb2e0] mb-1">⊞ Ejecutar con</div>
                <select
                  value={gameRunner}
                  onChange={(e) => changeGameRunner(e.target.value)}
                  className="w-full rounded-md bg-panel-2 border border-edge px-2 py-1.5 text-[13px] outline-none focus:border-neon/50"
                >
                  <option value="">Por defecto (Ajustes)</option>
                  {runners.map((r) => (
                    <option key={r.id} value={r.id}>
                      {r.label}
                    </option>
                  ))}
                </select>
                {runners.length === 0 && (
                  <div className="text-[11px] text-hot mt-1">
                    No se detectó Wine ni Proton (umu).
                  </div>
                )}
              </div>
            )}

            {/* ROM handling */}
            <div className="mt-3 pt-3 border-t border-edge text-[13px]">
              <div className="font-bold text-neon mb-1">
                ROM {project.rom.required ? "requerida" : "no necesaria"}
              </div>
              {romNone ? (
                <p className="text-white/55">{project.rom.notes}</p>
              ) : romInApp ? (
                <p className="text-white/55">{project.rom.notes}</p>
              ) : (
                project.installed && (
                  <div className="mt-1 flex items-center gap-2">
                    <button
                      onClick={pickRom}
                      disabled={busy}
                      className="text-sm rounded-md border border-neon/40 text-neon px-3 py-1.5 hover:bg-neon/10 disabled:opacity-50"
                    >
                      {project.rom_configured ? "Cambiar ROM…" : "Seleccionar ROM…"}
                    </button>
                    {project.rom_configured && <span className="text-[12px] text-neon-2">✓</span>}
                  </div>
                )
              )}
              {msg && <div className="mt-2 text-[12px] text-gold">{msg}</div>}
            </div>
          </div>

          <div className="rounded-xl border border-edge bg-panel p-4">
            <div className="text-xs font-bold tracking-widest text-white/40 mb-2">FICHA</div>
            <dl className="space-y-1.5 text-[13px]">
              {facts.map(([k, v]) => (
                <div key={k} className="flex justify-between gap-3">
                  <dt className="text-white/45">{k}</dt>
                  <dd className="text-white/85 text-right">{v}</dd>
                </div>
              ))}
            </dl>
            <div className="mt-3 pt-3 border-t border-edge flex gap-2">
              <button
                onClick={() => openUrl(repoUrl)}
                className="flex-1 rounded-lg border border-edge py-1.5 text-xs hover:border-neon/40"
              >
                GitHub
              </button>
              {wiki?.url && (
                <button
                  onClick={() => openUrl(wiki.url!)}
                  className="flex-1 rounded-lg border border-edge py-1.5 text-xs hover:border-neon/40"
                >
                  Wikipedia
                </button>
              )}
            </div>
          </div>
        </aside>
      </div>

      {lightbox && (
        <div
          className="fixed inset-0 z-50 bg-black/90 grid place-items-center p-8 cursor-zoom-out"
          onClick={() => setLightbox(null)}
        >
          <img src={lightbox} className="max-w-full max-h-full rounded-lg" />
        </div>
      )}
    </div>
  );
}
