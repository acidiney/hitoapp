import { readFileSync } from "node:fs";
import { resolve } from "node:path";

export function stylesheet(name: string): string {
  return readFileSync(resolve(__dirname, "../styles", name), "utf8");
}

function declarations(block: string): Record<string, string> {
  const result: Record<string, string> = {};
  for (const match of block.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g))
    result[match[1]] = match[2].trim();
  return result;
}

export function tokens(): {
  light: Record<string, string>;
  dark: Record<string, string>;
} {
  const css = stylesheet("base.css");
  const light = declarations(css.match(/:root\s*{([^}]*)}/)![1]);
  const darkMedia = css.match(
    /@media \(prefers-color-scheme: dark\)\s*{\s*:root\s*{([^}]*)}/,
  )![1];
  return { light, dark: { ...light, ...declarations(darkMedia) } };
}

export function rule(css: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const matches = [...css.matchAll(new RegExp(`${escaped}\\s*{([^}]*)}`, "g"))];
  if (matches.length === 0) throw new Error(`No rule for ${selector}`);
  return matches.map((match) => match[1]).join("\n");
}

function channel(value: number): number {
  const unit = value / 255;
  return unit <= 0.03928 ? unit / 12.92 : ((unit + 0.055) / 1.055) ** 2.4;
}

export function luminance(hex: string): number {
  const [r, g, b] = [1, 3, 5].map((start) =>
    channel(parseInt(hex.slice(start, start + 2), 16)),
  );
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

export function contrast(a: string, b: string): number {
  const [high, low] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (high + 0.05) / (low + 0.05);
}
