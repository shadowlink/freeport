import { useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { ProjectView, Runner, SystemInfo, WikiInfo } from "../types";
import { api } from "../api";
import { gradientFor, initials, screenshotsFrom } from "../lib/art";
import Icon from "../lib/icons";
import ModsPanel from "./ModsPanel";

interface Props {
  project: ProjectView;
  system?: SystemInfo;
  logoUrl?: string;
  allProjects: ProjectView[];
  onSelect: (p: ProjectView) => void;
  onClose: () => void;
  onChanged: () => void;
  onLaunch: () => void;
  onInstall: () => void;
}

export default function GamePage({
  project,
  system,
  logoUrl,
  allProjects,
  onSelect,
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
  const [lightbox, setLightbox] = useState<number | null>(null);
  const [runners, setRunners] = useState<Runner[]>([]);
  const [gameRunner, setGameRunner] = useState<string>("");

  const romInApp = project.rom.mode === "in-app";
  const romNone = project.rom.mode === "none";
  const gallery = useMemo(
    () => screenshotsFrom(project.cover_url).filter((s) => !failed.has(s)),
    [project.cover_url, failed],
  );
  const heroBg = gallery[0] ?? project.cover_url ?? undefined;

  const related = useMemo(() => {
    const same = allProjects.filter((p) => p.id !== project.id && p.system === project.system);
    return same.slice(0, 12);
  }, [allProjects, project.id, project.system]);

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

  // Lightbox keyboard nav.
  useEffect(() => {
    if (lightbox === null) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setLightbox(null);
      if (e.key === "ArrowRight") setLightbox((i) => (i === null ? i : (i + 1) % gallery.length));
      if (e.key === "ArrowLeft")
        setLightbox((i) => (i === null ? i : (i - 1 + gallery.length) % gallery.length));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [lightbox, gallery.length]);

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
  const publishedYear = project.cached?.published_at
    ? project.cached.published_at.slice(0, 4)
    : null;

  const facts: [string, string][] = [];
  facts.push(["Sistema", system?.name ?? project.system.toUpperCase()]);
  facts.push(["Tipo", project.type === "recompilation" ? "Recompilación" : "Port nativo"]);
  if (project.installed_tag) facts.push(["Versión instalada", project.installed_tag]);
  else if (project.cached?.latest_tag) facts.push(["Última versión", project.cached.latest_tag]);
  if (publishedYear) facts.push(["Publicado", publishedYear]);

  const Chip = ({ children, style }: { children: React.ReactNode; style?: React.CSSProperties }) => (
    <span
      className="text-[12px] font-semibold px-2.5 py-1 rounded-full border border-white/15 bg-black/35 backdrop-blur-sm"
      style={style}
    >
      {children}
    </span>
  );

  return (
    <div className="fixed inset-0 z-40 bg-void overflow-y-auto thin-scroll page-in">
      {/* Hero */}
      <div className="relative h-[46vh] min-h-[360px]">
        <div
          className="absolute inset-0 bg-cover bg-center scale-105 blur-[1px]"
          style={heroBg ? { backgroundImage: `url(${heroBg})` } : { background: gradientFor(project.id) }}
        />
        <div className="absolute inset-0 bg-gradient-to-t from-void via-void/75 to-void/20" />
        <div className="absolute inset-0 bg-gradient-to-r from-void/70 to-transparent" />

        <button
          onClick={onClose}
          className="absolute top-4 left-4 z-10 rounded-lg bg-black/50 border border-white/15 px-3 py-1.5 text-sm hover:bg-black/70 inline-flex items-center gap-1"
        >
          <Icon.ChevronLeft className="w-4 h-4" /> Volver
        </button>

        <div className="absolute bottom-0 left-0 right-0 p-8 flex items-end gap-6">
          <div
            className="w-40 shrink-0 aspect-[3/4] rounded-xl border border-white/10 bg-panel-2 poster shadow-2xl overflow-hidden grid place-items-center"
            style={
              project.cover_url
                ? { backgroundImage: `url(${project.cover_url})` }
                : { background: gradientFor(project.id) }
            }
          >
            {!project.cover_url && (
              <span className="text-4xl font-black text-white/85">
                {initials(project.original_game || project.name)}
              </span>
            )}
          </div>
          <div className="min-w-0 pb-1">
            {logoUrl && <img src={logoUrl} alt="" className="h-7 mb-2 object-contain object-left" />}
            <h1 className="text-4xl font-black leading-tight drop-shadow uppercase">
              {project.original_game}
            </h1>
            <div className="text-neon font-semibold text-lg">{project.name}</div>
            <div className="mt-3 flex flex-wrap items-center gap-2">
              {system && (
                <Chip style={{ color: system.color, borderColor: system.color + "80" }}>
                  {system.name}
                </Chip>
              )}
              <Chip>{project.type === "recompilation" ? "Recompilación" : "Port nativo"}</Chip>
              {project.year && <Chip>{project.year}</Chip>}
              {project.developer && <Chip>{project.developer}</Chip>}
              {project.genre && <Chip>{project.genre}</Chip>}
              {project.is_windows && (
                <Chip style={{ color: "#7fb2e0", borderColor: "#2b6fb3" }}>Windows · Wine/Proton</Chip>
              )}
            </div>
          </div>
        </div>
      </div>

      {/* Body */}
      <div className="max-w-6xl mx-auto px-6 py-6 grid grid-cols-1 lg:grid-cols-[1fr_320px] gap-8">
        {/* Main column */}
        <div className="space-y-7 min-w-0">
          {project.is_windows && (
            <div className="rounded-lg border border-[#2b6fb3]/60 bg-[#2b6fb3]/15 px-4 py-3 text-sm flex items-start gap-2">
              <Icon.Windows className="w-4 h-4 text-[#7fb2e0] shrink-0 mt-0.5" />
              <span className="text-white/80">
                Solo hay versión de Windows: se ejecuta con Wine/Proton.
              </span>
            </div>
          )}

          {gallery.length > 0 && (
            <section>
              <h2 className="text-lg font-black mb-3 uppercase tracking-wide">Capturas</h2>
              <div className="flex gap-3 overflow-x-auto thin-scroll pb-1">
                {gallery.map((s, i) => (
                  <img
                    key={s}
                    src={s}
                    loading="lazy"
                    decoding="async"
                    onError={() => setFailed((f) => new Set(f).add(s))}
                    onClick={() => setLightbox(i)}
                    className="h-52 rounded-lg border border-edge object-cover cursor-zoom-in shrink-0 hover:border-neon/50 transition-colors"
                  />
                ))}
              </div>
            </section>
          )}

          {(wikiLoading || wiki?.extract) && (
            <section>
              <h2 className="text-lg font-black mb-2 uppercase tracking-wide">Acerca del juego</h2>
              {wikiLoading && !wiki?.extract ? (
                <div className="text-white/40 text-sm">Cargando…</div>
              ) : (
                <div className="flex gap-4">
                  {wiki?.thumbnail && (
                    <img
                      src={wiki.thumbnail}
                      alt=""
                      className="w-28 shrink-0 rounded-lg border border-edge object-cover self-start hidden sm:block"
                    />
                  )}
                  <div>
                    <p className="text-[15px] leading-relaxed text-white/80 whitespace-pre-line">
                      {wiki!.extract}
                    </p>
                    <div className="mt-2 text-[11px] text-white/35">
                      Texto de{" "}
                      <button
                        onClick={() => wiki?.url && openUrl(wiki.url)}
                        className="underline hover:text-white/60"
                      >
                        Wikipedia
                      </button>{" "}
                      (CC BY-SA).
                    </div>
                  </div>
                </div>
              )}
            </section>
          )}

          {project.rom.notes && (
            <section>
              <h2 className="text-lg font-black mb-2 uppercase tracking-wide">Sobre este port</h2>
              <p className="text-[14px] leading-relaxed text-white/70">{project.rom.notes}</p>
            </section>
          )}

          {project.mods && <ModsPanel projectId={project.id} />}

          {related.length > 0 && (
            <section>
              <h2 className="text-lg font-black mb-3 uppercase tracking-wide">
                Más de {system?.name ?? project.system.toUpperCase()}
              </h2>
              <div className="flex gap-3 overflow-x-auto thin-scroll pb-2">
                {related.map((r) => (
                  <button
                    key={r.id}
                    onClick={() => onSelect(r)}
                    className="w-[120px] shrink-0 text-left group"
                    title={r.original_game}
                  >
                    <div
                      className="aspect-[3/4] rounded-lg border border-edge overflow-hidden bg-panel-2 poster grid place-items-center group-hover:border-neon/50 transition-colors"
                      style={
                        r.cover_url
                          ? { backgroundImage: `url(${r.cover_url})` }
                          : { background: gradientFor(r.id) }
                      }
                    >
                      {!r.cover_url && (
                        <span className="text-2xl font-black text-white/80">
                          {initials(r.original_game || r.name)}
                        </span>
                      )}
                    </div>
                    <div className="text-[11px] mt-1 line-clamp-2 text-white/70 group-hover:text-white">
                      {r.original_game || r.name}
                    </div>
                  </button>
                ))}
              </div>
            </section>
          )}
        </div>

        {/* Sidebar */}
        <aside className="space-y-4 lg:sticky lg:top-4 self-start">
          <div className="rounded-xl border border-edge bg-panel p-4">
            {project.installed ? (
              <div className="space-y-2">
                <button
                  onClick={onLaunch}
                  disabled={busy}
                  className="w-full rounded-lg bg-neon text-void font-black py-3 text-lg hover:brightness-110 disabled:opacity-50 inline-flex items-center justify-center gap-2"
                >
                  <Icon.Play className="w-5 h-5" /> Jugar
                </button>
                {project.update_available && (
                  <button
                    onClick={onInstall}
                    className="w-full rounded-lg border border-gold/50 text-gold py-2 hover:bg-gold/10 inline-flex items-center justify-center gap-2"
                  >
                    <Icon.ArrowUp className="w-4 h-4" /> Actualizar
                  </button>
                )}
                <button
                  onClick={uninstall}
                  disabled={busy}
                  className="w-full rounded-lg border border-hot/50 text-hot py-2 hover:bg-hot/10 disabled:opacity-50 inline-flex items-center justify-center gap-2"
                >
                  <Icon.Trash className="w-4 h-4" /> Eliminar
                </button>
              </div>
            ) : (
              <button
                onClick={onInstall}
                disabled={busy}
                className="w-full rounded-lg bg-neon text-void font-black py-3 text-lg hover:brightness-110 disabled:opacity-50 inline-flex items-center justify-center gap-2"
              >
                <Icon.Download className="w-5 h-5" /> Instalar
              </button>
            )}

            {project.is_windows && (
              <div className="mt-3 pt-3 border-t border-edge text-[13px]">
                <div className="font-bold text-[#7fb2e0] mb-1 inline-flex items-center gap-1.5">
                  <Icon.Windows className="w-3.5 h-3.5" /> Ejecutar con
                </div>
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
                  <div className="text-[11px] text-hot mt-1">No se detectó Wine ni Proton.</div>
                )}
              </div>
            )}

            {/* ROM handling */}
            <div className="mt-3 pt-3 border-t border-edge text-[13px]">
              <div className="font-bold text-neon mb-1">
                ROM {project.rom.required ? "requerida" : "no necesaria"}
              </div>
              {romNone || romInApp ? (
                <p className="text-white/55">{project.rom.notes}</p>
              ) : (
                <>
                  {project.rom.expected_filename && (
                    <p className="text-[12px] text-white/45 mb-1">
                      Archivo: <code className="text-white/70">{project.rom.expected_filename}</code>
                    </p>
                  )}
                  {project.installed && (
                    <div className="mt-1 flex items-center gap-2">
                      <button
                        onClick={pickRom}
                        disabled={busy}
                        className="text-sm rounded-md border border-neon/40 text-neon px-3 py-1.5 hover:bg-neon/10 disabled:opacity-50"
                      >
                        {project.rom_configured ? "Cambiar ROM…" : "Seleccionar ROM…"}
                      </button>
                      {project.rom_configured && <Icon.Check className="w-4 h-4 text-neon-2" />}
                    </div>
                  )}
                </>
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
            {project.cached?.platforms && project.cached.platforms.length > 0 && (
              <div className="mt-2 flex flex-wrap gap-1">
                {project.cached.platforms.map((p) => (
                  <span
                    key={p}
                    className="text-[10px] px-1.5 py-0.5 rounded bg-panel-2 border border-edge text-white/50"
                  >
                    {p}
                  </span>
                ))}
              </div>
            )}
            <div className="mt-3 pt-3 border-t border-edge flex gap-2">
              <button
                onClick={() => openUrl(repoUrl)}
                className="flex-1 rounded-lg border border-edge py-1.5 text-xs hover:border-neon/40 inline-flex items-center justify-center gap-1.5"
              >
                <Icon.ExternalLink className="w-3.5 h-3.5" /> GitHub
              </button>
              {wiki?.url && (
                <button
                  onClick={() => openUrl(wiki.url!)}
                  className="flex-1 rounded-lg border border-edge py-1.5 text-xs hover:border-neon/40 inline-flex items-center justify-center gap-1.5"
                >
                  <Icon.ExternalLink className="w-3.5 h-3.5" /> Wikipedia
                </button>
              )}
            </div>
          </div>
        </aside>
      </div>

      {/* Lightbox with prev/next + keyboard nav */}
      {lightbox !== null && gallery[lightbox] && (
        <div
          className="fixed inset-0 z-50 bg-black/92 grid place-items-center p-8"
          onClick={() => setLightbox(null)}
        >
          <img src={gallery[lightbox]} className="max-w-full max-h-full rounded-lg" />
          {gallery.length > 1 && (
            <>
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  setLightbox((i) => (i === null ? i : (i - 1 + gallery.length) % gallery.length));
                }}
                className="absolute left-4 top-1/2 -translate-y-1/2 w-11 h-11 grid place-items-center rounded-full bg-black/60 border border-white/20 hover:bg-black/80"
              >
                <Icon.ChevronLeft className="w-6 h-6" />
              </button>
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  setLightbox((i) => (i === null ? i : (i + 1) % gallery.length));
                }}
                className="absolute right-4 top-1/2 -translate-y-1/2 w-11 h-11 grid place-items-center rounded-full bg-black/60 border border-white/20 hover:bg-black/80"
              >
                <Icon.ChevronRight className="w-6 h-6" />
              </button>
              <div className="absolute bottom-6 left-1/2 -translate-x-1/2 text-sm text-white/60">
                {lightbox + 1} / {gallery.length}
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}
