# Freeport

Launcher de escritorio, portable y "consolero", para descubrir, instalar,
mantener actualizados y **lanzar** los ports y recompilaciones a PC de juegos de
consola (Ship of Harkinian, Zelda 64: Recompiled, Perfect Dark PC, etc.),
clasificados por sistema.

Stack: **Tauri v2** (Rust) + **React + TypeScript + Vite** + **Tailwind v4**.
Objetivo de plataformas: Linux (primero), Windows, y Android en el futuro.

## La realidad que da forma a la app

Las listas de la comunidad
([awesome-game-decompilations](https://github.com/CharlotteCross1998/awesome-game-decompilations)
y [Game-Decompilations](https://github.com/SamidyFR/Game-Decompilations)) son
Markdown plano con ~1000 entradas, pero **la mayoría son decompilaciones
"matching" que no producen ejecutable** (al compilar reproducen el ROM original).
Solo son *lanzables* dos categorías: **ports nativos** que publican binario y
**recompilaciones estáticas**. Y casi todas, aun con binario, **requieren que
aportes tu propio ROM legal** (en el primer arranque o al compilar).

Por eso Freeport no consume las listas en vivo: se apoya en un **manifiesto
curado** (`src-tauri/catalog.seed.json`) de proyectos realmente lanzables, con
reglas por proyecto para elegir el binario correcto de tu plataforma. Las listas
se usan solo como fuente de descubrimiento (`tools/discover.py`). **La app nunca
distribuye ROMs.**

## Arquitectura

```
src-tauri/
  catalog.seed.json     # manifiesto curado embebido (fallback offline)
  src/
    model.rs            # esquema del catálogo + estado instalado + config
    platform.rs         # triple actual (linux-x86_64, windows-x86_64, …)
    store.rs            # rutas, modo portable, persistencia JSON, carga de catálogo
    github.rs           # releases (/releases, no /latest) + selección de asset por regex
    install.rs          # descarga con progreso + extracción (zip/tar.gz) + localizar binario
    launch.rs           # spawn del binario con cwd = carpeta de instalación
    commands.rs         # comandos Tauri (list_catalog, install, launch, set_rom, …)
    lib.rs              # wiring + fix WebKit/Wayland
src/                    # frontend React (App, GameCard, DetailModal, Settings)
tools/
  probe.py             # rellena cached.{platforms,latest_tag,…} (lo corre la CI del catálogo)
  discover.py          # cruza las listas de la comunidad con repos que publican binarios
```

Puntos clave de diseño:

- **Filtrado por plataforma:** se ocultan los proyectos sin binario para tu
  `os-arch`, usando `cached.platforms` (rellenado por `probe.py`) o, si falta,
  la presencia de una `asset_rules[triple]`.
- **Rate limit de GitHub:** la CI del catálogo sondea (60/h sin token, 5000 con
  token) y cachea versión + plataformas; la app solo llama a GitHub al
  instalar/actualizar. Se puede configurar un token en Ajustes. Endpoint
  `/releases?per_page=N` (no `/latest`, que da 404 con proyectos que solo
  publican prereleases). Para tags rolling (`ci-dev-build`) se compara
  `published_at`.
- **Modo portable:** si hay un `portable.txt` junto al ejecutable, todo el estado
  vive en `data/` al lado del binario; si no, en el directorio de datos del SO.

## Requisitos

- Rust (rustup) y Node 18+.
- Linux: `webkit2gtk-4.1`, `gtk3`, `librsvg` (en Arch/CachyOS ya suelen estar).

## Ejecutar en desarrollo

```bash
npm install
npm run tauri dev
```

> Nota Linux/Nvidia: WebKitGTK+Wayland puede fallar con «Error 71 dispatching to
> Wayland display». La app ya exporta `WEBKIT_DISABLE_DMABUF_RENDERER=1` al
> arrancar (ver `lib.rs`), así que no hace falta hacer nada; si aún así falla,
> prueba `GDK_BACKEND=x11 npm run tauri dev`.

## Compilar un ejecutable

```bash
npm run tauri build
```

Produce, sin necesidad de servidor de desarrollo:

- **Ejecutable standalone:** `src-tauri/target/release/Freeport` (~23 MB).
  Arranca por doble clic; usa el `webkit2gtk` del sistema.
- **AppImage portable:** `src-tauri/target/release/bundle/appimage/Freeport_0.1.0_amd64.AppImage`
  (~105 MB). Un único archivo autocontenido; ideal para llevarlo en un USB.

> En Arch/CachyOS el empaquetado del AppImage falla con «failed to run
> linuxdeploy» porque el `strip` que trae linuxdeploy no entiende la sección
> ELF `.relr.dyn` del toolchain moderno. Compílalo saltándote el strip (y con el
> workaround de FUSE anidado):
>
> ```bash
> NO_STRIP=1 APPIMAGE_EXTRACT_AND_RUN=1 npm run tauri build
> ```
>
> Si un intento falló a medias, borra `src-tauri/target/release/bundle/appimage`
> antes de reintentar.

## Modo TV / Big Picture (salón + Moonlight)

Freeport incluye un modo "Big Picture" a pantalla completa, navegable con mando
(hero + estanterías de carátulas grandes). Para entrar: botón **📺 Modo TV** en la
barra lateral, o arrancar con el flag `--tv`. Para salir: **Start** en el mando o
`Esc`. También se maneja con teclado (flechas / Enter / Esc / RePág-AvPág).

- **Mando:** se lee en Rust con `gilrs` (evdev), así que funciona con mandos
  locales y con el pad virtual que reenvía **Moonlight**.
- **Jugar → volver:** al lanzar un juego se muestra un overlay "Jugando…"; cuando
  el proceso del juego termina, el launcher recupera el foco a pantalla completa.

### Añadir a Sunshine (para jugar por Moonlight desde la TV)

En **Ajustes → Sunshine / Moonlight** pulsa **"Añadir a Sunshine"**: registra
Freeport en `~/.config/sunshine/apps.json` (con backup) con el comando
`<AppImage> --tv`. Reinicia Sunshine y aparecerá en Moonlight; al abrirlo por
streaming arranca directo en Modo TV y se maneja con el mando del cliente.

## Tests

```bash
cd src-tauri
cargo test                 # unit + integración (extracción, selección de asset, seed, filtrado)
cargo test -- --ignored    # e2e real: descarga un release real y localiza el binario
```

## Mantener el catálogo

```bash
GITHUB_TOKEN=ghp_xxx python3 tools/probe.py src-tauri/catalog.seed.json     # actualiza cached
GITHUB_TOKEN=ghp_xxx python3 tools/discover.py --check --limit 200 > candidates.json
```

Lo natural es mover el catálogo a su propio repo (`freeport-catalog`) con un
workflow diario que ejecute `probe.py` y publique `catalog.json`; la app lo
descargaría vía la "URL del catálogo remoto" de Ajustes.

## Estado

MVP funcional en Linux: navegación filtrada por plataforma, instalación
(descarga+extracción), lanzamiento, asistente de ROM, detección de
actualizaciones (botón + `probe.py`), token opcional y modo portable.
Pendiente/roadmap: peticiones condicionales con ETag, empaquetado Windows,
navegación con mando, integración con ES-DE, y build de Android.
