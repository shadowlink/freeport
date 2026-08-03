import { getCurrentWindow } from "@tauri-apps/api/window";
import Icon from "../lib/icons";

const win = () => getCurrentWindow();

// Custom titlebar: draggable brand bar + window controls. Replaces the OS
// decorations (decorations:false). Hidden in TV/fullscreen mode.
export default function TitleBar({
  section,
  maximized,
}: {
  section?: string;
  maximized: boolean;
}) {
  return (
    <div
      data-tauri-drag-region
      className="h-10 shrink-0 flex items-center gap-3 pl-3 pr-1 border-b border-edge bg-panel/70 select-none"
      onDoubleClick={() => win().toggleMaximize().catch(() => {})}
    >
      <div data-tauri-drag-region className="flex items-center gap-2 pointer-events-none">
        <span className="w-6 h-6 rounded-md grid place-items-center bg-neon/15 text-neon">
          <Icon.Anchor className="w-4 h-4" />
        </span>
        <span className="font-black tracking-widest text-[13px]">
          FREE<span className="text-neon">PORT</span>
        </span>
      </div>
      {section && (
        <span data-tauri-drag-region className="text-[12px] text-white/35 pointer-events-none">
          / {section}
        </span>
      )}

      <div className="ml-auto flex items-center">
        <button
          onClick={() => win().minimize().catch(() => {})}
          className="w-11 h-10 grid place-items-center text-white/60 hover:text-white hover:bg-white/5"
          title="Minimizar"
        >
          <Icon.Minimize className="w-4 h-4" />
        </button>
        <button
          onClick={() => win().toggleMaximize().catch(() => {})}
          className="w-11 h-10 grid place-items-center text-white/60 hover:text-white hover:bg-white/5"
          title={maximized ? "Restaurar" : "Maximizar"}
        >
          {maximized ? <Icon.Restore className="w-4 h-4" /> : <Icon.Maximize className="w-3.5 h-3.5" />}
        </button>
        <button
          onClick={() => win().close().catch(() => {})}
          className="w-11 h-10 grid place-items-center text-white/60 hover:text-white hover:bg-hot"
          title="Cerrar"
        >
          <Icon.Close className="w-4 h-4" />
        </button>
      </div>
    </div>
  );
}

// Thin invisible resize handles for the borderless window.
const EDGES: { dir: string; cls: string }[] = [
  { dir: "North", cls: "top-0 left-2 right-2 h-1 cursor-ns-resize" },
  { dir: "South", cls: "bottom-0 left-2 right-2 h-1 cursor-ns-resize" },
  { dir: "West", cls: "top-2 bottom-2 left-0 w-1 cursor-ew-resize" },
  { dir: "East", cls: "top-2 bottom-2 right-0 w-1 cursor-ew-resize" },
  { dir: "NorthWest", cls: "top-0 left-0 w-2 h-2 cursor-nwse-resize" },
  { dir: "NorthEast", cls: "top-0 right-0 w-2 h-2 cursor-nesw-resize" },
  { dir: "SouthWest", cls: "bottom-0 left-0 w-2 h-2 cursor-nesw-resize" },
  { dir: "SouthEast", cls: "bottom-0 right-0 w-2 h-2 cursor-nwse-resize" },
];

export function ResizeHandles() {
  return (
    <>
      {EDGES.map((e) => (
        <div
          key={e.dir}
          className={`resize-edge ${e.cls}`}
          onMouseDown={(ev) => {
            if (ev.button !== 0) return;
            // eslint-disable-next-line @typescript-eslint/no-explicit-any
            win().startResizeDragging(e.dir as any).catch(() => {});
          }}
        />
      ))}
    </>
  );
}
