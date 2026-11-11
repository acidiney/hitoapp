export type PluginTone = { background: string; color: string };

const KNOWN: Record<string, PluginTone> = {
  youtube: { background: "#5a2a4f", color: "#e7a3c8" },
  spotify: { background: "#1f4a43", color: "#8fd9c4" },
};
const PALETTE: PluginTone[] = [
  { background: "#2a3a5c", color: "#a9bcf0" },
  { background: "#4a3a1f", color: "#e8c98a" },
  { background: "#3d2f5a", color: "#c5b0ee" },
  { background: "#3a4a22", color: "#bfdc8a" },
];

export function pluginTone(name: string): PluginTone {
  const key = name.trim().toLowerCase().split(/\s+/)[0] ?? "";
  const known = KNOWN[key];
  if (known) return known;
  let hash = 0;
  for (const character of key)
    hash = (hash * 31 + character.charCodeAt(0)) >>> 0;
  return PALETTE[hash % PALETTE.length];
}

export function pluginInitial(name: string): string {
  return (name.trim().charAt(0) || "?").toUpperCase();
}
