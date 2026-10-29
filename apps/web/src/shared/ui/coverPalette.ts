export type CoverShape = "circle" | "band" | "plain";

export type CoverStyle = {
  background: string;
  foreground: string;
  accent: string;
  shape: CoverShape;
};

export const COVER_PALETTE = [
  { background: "#2f4bff", foreground: "#ffffff", accent: "#ffd400" },
  { background: "#2f6f6a", foreground: "#ffffff", accent: "#e8e2d4" },
  { background: "#6b3a52", foreground: "#ffffff", accent: "#f2c6d6" },
  { background: "#b5412b", foreground: "#ffffff", accent: "#ffd9b8" },
  { background: "#4d6b57", foreground: "#ffffff", accent: "#dfe8c9" },
  { background: "#141414", foreground: "#ffffff", accent: "#2f4bff" },
  { background: "#3a3f8f", foreground: "#ffffff", accent: "#ffb703" },
  { background: "#8a5a00", foreground: "#ffffff", accent: "#ffe29a" },
] as const;

const SHAPES: CoverShape[] = ["circle", "band", "plain"];

function fnv1a(text: string): number {
  let hash = 0x811c9dc5;
  for (let index = 0; index < text.length; index += 1) {
    hash ^= text.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash;
}

export function coverStyle(id: string): CoverStyle {
  const hash = fnv1a(id);
  const palette = COVER_PALETTE[hash % COVER_PALETTE.length];
  const shape = SHAPES[Math.floor(hash / COVER_PALETTE.length) % SHAPES.length];
  return { ...palette, shape };
}
