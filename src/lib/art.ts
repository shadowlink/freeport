// Deterministic placeholder cover art derived from a string, used until the
// catalog ships real cover_url artwork.

function hash(str: string): number {
  let h = 2166136261;
  for (let i = 0; i < str.length; i++) {
    h ^= str.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

export function gradientFor(seed: string): string {
  const h = hash(seed);
  const a = h % 360;
  const b = (a + 60 + (h % 80)) % 360;
  return `linear-gradient(135deg, hsl(${a} 70% 32%), hsl(${b} 65% 18%))`;
}

/// Derives libretro in-game screenshot + title-screen URLs from a boxart URL
/// (same server, different folder). Returns [] when there's no libretro cover.
export function screenshotsFrom(coverUrl: string | null): string[] {
  if (!coverUrl || !coverUrl.includes("/Named_Boxarts/")) return [];
  return [
    coverUrl.replace("/Named_Boxarts/", "/Named_Snaps/"),
    coverUrl.replace("/Named_Boxarts/", "/Named_Titles/"),
  ];
}

export function initials(name: string): string {
  const words = name.replace(/[^\p{L}\p{N} ]/gu, "").split(/\s+/).filter(Boolean);
  if (words.length === 1) return words[0].slice(0, 2).toUpperCase();
  return (words[0][0] + words[words.length - 1][0]).toUpperCase();
}
