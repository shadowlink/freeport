// Inline SVG icon set (lucide-style), bundled — no runtime dependency.
// Usage: <Icon.Play className="w-4 h-4" />
import type { SVGProps } from "react";

type P = SVGProps<SVGSVGElement> & { size?: number };

function Svg({ size = 24, children, ...p }: P & { children: React.ReactNode }) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      {...p}
    >
      {children}
    </svg>
  );
}

export const Icon = {
  Anchor: (p: P) => (
    <Svg {...p}>
      <circle cx="12" cy="5" r="2.5" />
      <path d="M12 7.5V22" />
      <path d="M5 12H3a9 9 0 0 0 18 0h-2" />
      <path d="M8.5 9.5 12 7l3.5 2.5" />
    </Svg>
  ),
  Settings: (p: P) => (
    <Svg {...p}>
      <path d="M12.2 2h-.4a2 2 0 0 0-2 2 1.7 1.7 0 0 1-2.5 1.5 2 2 0 0 0-2.7.7l-.2.4a2 2 0 0 0 .7 2.7A1.7 1.7 0 0 1 4 12a1.7 1.7 0 0 1-.9 1.5 2 2 0 0 0-.7 2.7l.2.4a2 2 0 0 0 2.7.7A1.7 1.7 0 0 1 8 18.9a2 2 0 0 0 2 2h.4a2 2 0 0 0 2-2 1.7 1.7 0 0 1 2.5-1.5 2 2 0 0 0 2.7-.7l.2-.4a2 2 0 0 0-.7-2.7A1.7 1.7 0 0 1 20 12a1.7 1.7 0 0 1 .9-1.5 2 2 0 0 0 .7-2.7l-.2-.4a2 2 0 0 0-2.7-.7A1.7 1.7 0 0 1 16 5.1a2 2 0 0 0-2-2Z" />
      <circle cx="12" cy="12" r="2.6" />
    </Svg>
  ),
  Tv: (p: P) => (
    <Svg {...p}>
      <rect x="2" y="7" width="20" height="13" rx="2" />
      <path d="m8 3 4 4 4-4" />
    </Svg>
  ),
  Play: (p: P) => (
    <Svg {...p} fill="currentColor" stroke="none">
      <path d="M6 4.5v15a1 1 0 0 0 1.5.87l12-7.5a1 1 0 0 0 0-1.74l-12-7.5A1 1 0 0 0 6 4.5Z" />
    </Svg>
  ),
  Download: (p: P) => (
    <Svg {...p}>
      <path d="M12 3v12" />
      <path d="m7 11 5 5 5-5" />
      <path d="M4 20h16" />
    </Svg>
  ),
  Search: (p: P) => (
    <Svg {...p}>
      <circle cx="11" cy="11" r="7" />
      <path d="m21 21-4.3-4.3" />
    </Svg>
  ),
  Minimize: (p: P) => (
    <Svg {...p}>
      <path d="M5 12h14" />
    </Svg>
  ),
  Maximize: (p: P) => (
    <Svg {...p}>
      <rect x="5" y="5" width="14" height="14" rx="1.5" />
    </Svg>
  ),
  Restore: (p: P) => (
    <Svg {...p}>
      <rect x="7" y="7" width="11" height="11" rx="1.5" />
      <path d="M9 7V5.5A1.5 1.5 0 0 1 10.5 4H19a1.5 1.5 0 0 1 1.5 1.5V14a1.5 1.5 0 0 1-1.5 1.5H17.5" />
    </Svg>
  ),
  Close: (p: P) => (
    <Svg {...p}>
      <path d="M6 6l12 12M18 6 6 18" />
    </Svg>
  ),
  ChevronLeft: (p: P) => (
    <Svg {...p}>
      <path d="m15 18-6-6 6-6" />
    </Svg>
  ),
  ChevronRight: (p: P) => (
    <Svg {...p}>
      <path d="m9 18 6-6-6-6" />
    </Svg>
  ),
  Windows: (p: P) => (
    <Svg {...p} fill="currentColor" stroke="none">
      <path d="M3 5.5 10.5 4.4v7.1H3zM12 4.2 21 3v8.5h-9zM3 12.5h7.5v7.1L3 18.5zM12 12.5h9V21l-9-1.2z" />
    </Svg>
  ),
  Check: (p: P) => (
    <Svg {...p}>
      <path d="m20 6-11 11-5-5" />
    </Svg>
  ),
  Refresh: (p: P) => (
    <Svg {...p}>
      <path d="M21 12a9 9 0 1 1-2.6-6.4" />
      <path d="M21 3v5h-5" />
    </Svg>
  ),
  ExternalLink: (p: P) => (
    <Svg {...p}>
      <path d="M15 3h6v6" />
      <path d="M10 14 21 3" />
      <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
    </Svg>
  ),
  Trash: (p: P) => (
    <Svg {...p}>
      <path d="M4 7h16" />
      <path d="M9 7V5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2" />
      <path d="M6 7l1 13a1 1 0 0 0 1 1h8a1 1 0 0 0 1-1l1-13" />
    </Svg>
  ),
  ArrowUp: (p: P) => (
    <Svg {...p}>
      <path d="M12 20V6" />
      <path d="m5 11 7-7 7 7" />
    </Svg>
  ),
  Gamepad: (p: P) => (
    <Svg {...p}>
      <path d="M7 8h10a4 4 0 0 1 4 4l-.7 4.2A2.5 2.5 0 0 1 14.9 16l-.9-1H10l-.9 1a2.5 2.5 0 0 1-4.4-.8L4 12a4 4 0 0 1 3-4Z" />
      <path d="M8 11v2M7 12h2M15.5 11.5h.01M17.5 13.5h.01" />
    </Svg>
  ),
  Library: (p: P) => (
    <Svg {...p}>
      <path d="M4 4v16" />
      <path d="M9 4v16" />
      <path d="m14 5 5 15" />
    </Svg>
  ),
  Grid: (p: P) => (
    <Svg {...p}>
      <rect x="4" y="4" width="7" height="7" rx="1" />
      <rect x="13" y="4" width="7" height="7" rx="1" />
      <rect x="4" y="13" width="7" height="7" rx="1" />
      <rect x="13" y="13" width="7" height="7" rx="1" />
    </Svg>
  ),
};

export default Icon;
