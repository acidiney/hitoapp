import { oklchToHex } from "./coverColor";

export type CoverArtKind =
  | "album"
  | "artist"
  | "playlist"
  | "podcast"
  | "audiobook"
  | "track";

type PaintKind = Exclude<CoverArtKind, "track">;

export const COVER_HUES = [
  18, 36, 58, 88, 130, 165, 195, 215, 240, 268, 300, 335,
] as const;

const ACCENT_OFFSETS = [70, 120, 160] as const;
const LIGHTNESS_TIERS = [0.46, 0.56] as const;
const MOTIFS_PER_KIND = 6;
const STAGE_HEIGHT = 52;
const TEXT_TOP = 60;
const PAINT_CACHE_LIMIT = 4000;

export type CoverPaint = {
  key: string;
  kind: PaintKind;
  hue: number;
  tier: 0 | 1;
  base: string;
  deep: string;
  accent: string;
  accentSoft: string;
  motif: number;
  orientation: 0 | 1 | 2 | 3;
  variant: number;
};

export type CoverTextScale = "large" | "medium" | "small";

export type CoverArtSpec = {
  paint: CoverPaint;
  layout: { motifHeight: number; textTop: number };
  text: { scale: CoverTextScale; title: string; subtitle: string | null };
  monogram: string;
};

function fnv1a(text: string): number {
  let hash = 0x811c9dc5;
  for (let index = 0; index < text.length; index += 1) {
    hash ^= text.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash;
}

function avalanche(value: number): number {
  let hash = value;
  hash ^= hash >>> 16;
  hash = Math.imul(hash, 0x85ebca6b) >>> 0;
  hash ^= hash >>> 13;
  hash = Math.imul(hash, 0xc2b2ae35) >>> 0;
  hash ^= hash >>> 16;
  return hash >>> 0;
}

function paintKind(kind: CoverArtKind): PaintKind {
  return kind === "track" ? "album" : kind;
}

function takeDigit(state: { rest: number }, radix: number): number {
  const digit = state.rest % radix;
  state.rest = Math.floor(state.rest / radix);
  return digit;
}

function buildPaint(kind: PaintKind, id: string): CoverPaint {
  const key = `${kind}:${id}`;
  const state = { rest: avalanche(fnv1a(key)) };
  const hue = COVER_HUES[takeDigit(state, COVER_HUES.length)];
  const offset = ACCENT_OFFSETS[takeDigit(state, ACCENT_OFFSETS.length)];
  const tier = takeDigit(state, LIGHTNESS_TIERS.length) as 0 | 1;
  const motif = takeDigit(state, MOTIFS_PER_KIND);
  const orientation = takeDigit(state, 4) as 0 | 1 | 2 | 3;
  const variant = takeDigit(state, 4);
  const warm = hue <= 60 || hue >= 330;
  const chroma = warm ? 0.14 : 0.11;
  const lightness = LIGHTNESS_TIERS[tier];
  const accentHue = (hue + offset) % 360;
  return {
    key,
    kind,
    hue,
    tier,
    base: oklchToHex(lightness, chroma, hue),
    deep: oklchToHex(lightness - 0.16, chroma * 0.75, hue),
    accent: oklchToHex(0.8, 0.13, accentHue),
    accentSoft: oklchToHex(0.66, 0.14, accentHue),
    motif,
    orientation,
    variant,
  };
}

const paints = new Map<string, CoverPaint>();

export function coverPaint(kind: CoverArtKind, id: string): CoverPaint {
  const resolved = paintKind(kind);
  const key = `${resolved}:${id}`;
  const cached = paints.get(key);
  if (cached) return cached;
  if (paints.size >= PAINT_CACHE_LIMIT) paints.clear();
  const created = buildPaint(resolved, id);
  paints.set(key, created);
  return created;
}

const WIDE = /[ᄀ-ᅟ⺀-꓏가-힣豈-﫿︰-﹯＀-｠￠-￦]|\p{Extended_Pictographic}/u;

function textWidth(title: string): number {
  let width = 0;
  for (const character of title) width += WIDE.test(character) ? 1.9 : 1;
  return width;
}

function textScale(title: string): CoverTextScale {
  const width = textWidth(title);
  if (width <= 14) return "large";
  return width <= 30 ? "medium" : "small";
}

let segmenter: Intl.Segmenter | undefined;

export function coverMonogram(title: string): string {
  segmenter ??= new Intl.Segmenter(undefined, { granularity: "grapheme" });
  for (const { segment } of segmenter.segment(title)) {
    if (/\p{L}|\p{N}/u.test(segment)) {
      const upper = segment.toUpperCase();
      return [...segmenter.segment(upper)].length === 1 ? upper : segment;
    }
    if (/\p{Extended_Pictographic}|\p{Regional_Indicator}/u.test(segment))
      return segment;
  }
  return "·";
}

export function coverArt(
  kind: CoverArtKind,
  id: string,
  title: string,
  subtitle: string | null | undefined,
): CoverArtSpec {
  return {
    paint: coverPaint(kind, id),
    layout: { motifHeight: STAGE_HEIGHT, textTop: TEXT_TOP },
    text: { scale: textScale(title), title, subtitle: subtitle || null },
    monogram: coverMonogram(title),
  };
}
