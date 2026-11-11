import type { Schema } from "../../../shared/schema";

type Capability = Schema<"Manifest">["capabilities"][number];

export type CapabilityRow = {
  key: string;
  name: string;
  what: string;
  providers: string[];
};

const ROWS: {
  key: string;
  name: string;
  what: string;
  capability: Capability;
}[] = [
  {
    key: "search",
    name: "Search",
    what: "Find music beyond your library.",
    capability: "search",
  },
  {
    key: "sources",
    name: "Playback",
    what: "Play tracks you have no file for.",
    capability: "sources",
  },
  {
    key: "metadata",
    name: "Track details",
    what: "Fill in missing titles, artists and albums.",
    capability: "metadata",
  },
  {
    key: "playlists",
    name: "Playlists",
    what: "Follow or copy a playlist from a link.",
    capability: "playlists",
  },
  {
    key: "artwork",
    name: "Covers",
    what: "Fetch album artwork.",
    capability: "artwork",
  },
  {
    key: "lyrics",
    name: "Lyrics",
    what: "Show synced lyrics in now playing.",
    capability: "lyrics",
  },
];

export function capabilityRows(
  installations: Schema<"PluginInstallation">[],
): CapabilityRow[] {
  return ROWS.map(({ key, name, what, capability }) => ({
    key,
    name,
    what,
    providers: installations
      .filter((plugin) => plugin.manifest.capabilities.includes(capability))
      .map((plugin) => plugin.manifest.name),
  }));
}

const BENEFITS: Partial<Record<Capability, { name: string; text: string }>> = {
  search: { name: "Search", text: "beyond your library, only when you ask" },
  sources: {
    name: "Playback",
    text: "for tracks with no file, after local files and saved copies",
  },
  metadata: { name: "Track details", text: "filled in for incomplete tags" },
  playlists: {
    name: "Playlists",
    text: "you can follow or copy by pasting a link",
  },
  artwork: { name: "Covers", text: "for albums with no artwork" },
  lyrics: { name: "Lyrics", text: "shown in now playing" },
  catalog: { name: "Browse", text: "albums and artists from the service" },
  featured: { name: "Featured", text: "music on Discover" },
};
const BENEFIT_ORDER: Capability[] = [
  "search",
  "sources",
  "metadata",
  "playlists",
  "artwork",
  "lyrics",
  "catalog",
  "featured",
];

export function connectionBenefits(capabilities: Capability[]) {
  return BENEFIT_ORDER.filter((value) => capabilities.includes(value)).map(
    (value) => BENEFITS[value]!,
  );
}

const EXTRA: Partial<Record<Capability, { name: string; what: string }>> = {
  catalog: {
    name: "Browse",
    what: "Lists albums and artists from the service.",
  },
  source_preparation: {
    name: "Faster playback",
    what: "Gets songs ready to play sooner.",
  },
  dashboard: { name: "Dashboard", what: "Shows the service's own page." },
  backing_tracks: { name: "Backing tracks", what: "Offers backing tracks." },
  featured: { name: "Featured", what: "Features music on Discover." },
};

export function declaredCapabilities(capabilities: Capability[]) {
  const rows = ROWS.filter((row) => capabilities.includes(row.capability)).map(
    ({ key, name, what }) => ({ key, name, what }),
  );
  const extra = (Object.keys(EXTRA) as Capability[])
    .filter((value) => capabilities.includes(value))
    .map((value) => ({ key: value, ...EXTRA[value]! }));
  return [...rows, ...extra];
}
