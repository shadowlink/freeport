// Console logos bundled with the app (src/assets/logos/<systemId>.svg), so the
// launcher is self-sufficient and never depends on an external app (ES-DE) being
// installed. Vite inlines these at build time and returns a URL per file.
const files = import.meta.glob("../assets/logos/*.svg", {
  eager: true,
  query: "?url",
  import: "default",
});

export const SYSTEM_LOGOS: Record<string, string> = {};
for (const [path, url] of Object.entries(files)) {
  const id = path.split("/").pop()!.replace(/\.svg$/, "");
  SYSTEM_LOGOS[id] = url as string;
}
