import { coverStyle } from "./coverPalette";

export function tintFor(id: string): string {
  return coverStyle(id).background;
}

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

export function contrastOnTint(tint: string, alpha: number): number {
  const base = [1, 3, 5].map((offset) => channel(tint, offset));
  const text = base.map((value) => value + (255 - value) * alpha);
  return (luminance(text) + 0.05) / (luminance(base) + 0.05);
}

export function headerCoverId(id: string): string {
  const tint = tintFor(id);
  for (let attempt = 1; ; attempt += 1) {
    const candidate = `${id}:cover${attempt}`;
    if (coverStyle(candidate).background !== tint) return candidate;
  }
}
