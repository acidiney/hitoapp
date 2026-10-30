import { coverStyle } from "./coverPalette";

/**
 * Flat header colour for artist and playlist pages. The server does not extract
 * a dominant colour from artwork, so the colour is the generated-cover palette
 * entry for a stable id (the artist or chosen-artist id, or the playlist id).
 * The same id always gives the same colour, and every palette colour keeps white
 * text at readable contrast.
 */
export function tintFor(id: string): string {
  return coverStyle(id).background;
}

/** Opacity of the supporting text (crumbs, eyebrow, stats) on a tinted header. */
export const HEADER_MUTED_ALPHA = 0.9;

function channel(hex: string, offset: number) {
  return parseInt(hex.slice(offset, offset + 2), 16);
}

function luminance(rgb: number[]) {
  const [r, g, b] = rgb.map((value) => {
    const unit = value / 255;
    return unit <= 0.03928 ? unit / 12.92 : ((unit + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** Contrast ratio of white text at `alpha` opacity over a tint colour. */
export function contrastOnTint(tint: string, alpha: number): number {
  const base = [1, 3, 5].map((offset) => channel(tint, offset));
  const text = base.map((value) => value + (255 - value) * alpha);
  return (luminance(text) + 0.05) / (luminance(base) + 0.05);
}

/**
 * Identity for the generated cover on a tinted header. The header colour is the
 * cover palette entry of the collection id, so a cover generated from the same
 * id would vanish into it. This returns an id whose generated cover is a
 * different palette entry, so its shape stays visible.
 */
export function headerCoverId(id: string): string {
  const tint = tintFor(id);
  for (let attempt = 1; ; attempt += 1) {
    const candidate = `${id}:cover${attempt}`;
    if (coverStyle(candidate).background !== tint) return candidate;
  }
}
