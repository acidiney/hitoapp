import { COVER_PALETTE } from "./coverPalette";

export type AvatarColours = { background: string; foreground: string };

function fnv1a(text: string): number {
  let hash = 0x811c9dc5;
  for (let index = 0; index < text.length; index += 1) {
    hash ^= text.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash;
}

const graphemes = new Intl.Segmenter(undefined, { granularity: "grapheme" });

function firstSymbol(name: string): string {
  const first = graphemes.segment(name.trim())[Symbol.iterator]().next();
  return first.done ? "?" : first.value.segment;
}

export function initials(name: string): string {
  const parts = name.split(/[^\p{L}\p{N}]+/u).filter(Boolean);
  if (parts.length === 0)
    return /\p{Extended_Pictographic}/u.test(name) ? firstSymbol(name) : "?";
  return parts
    .slice(0, 2)
    .map((part) => Array.from(part)[0].toUpperCase())
    .join("");
}

export function avatarColours(id: string): AvatarColours {
  const entry = COVER_PALETTE[fnv1a(`avatar:${id}`) % COVER_PALETTE.length];
  return { background: entry.background, foreground: entry.foreground };
}

function luminance(hex: string): number {
  const channels = [1, 3, 5].map((start) => {
    const value = parseInt(hex.slice(start, start + 2), 16) / 255;
    return value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2];
}

export function contrastRatio(first: string, second: string): number {
  const [light, dark] = [luminance(first), luminance(second)].sort(
    (a, b) => b - a,
  );
  return (light + 0.05) / (dark + 0.05);
}
