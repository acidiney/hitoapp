import type { Schema } from "../../../shared/schema";

type Capability = Schema<"Manifest">["capabilities"][number];

const NEW_LABELS: Record<Capability, string> = {
  search: "song search",
  catalog: "album and artist browsing",
  metadata: "track details",
  sources: "playback",
  source_preparation: "faster playback",
  playlists: "playlist import",
  artwork: "album covers",
  dashboard: "a service dashboard",
  lyrics: "lyrics",
  backing_tracks: "backing tracks",
  featured: "featured music on Discover",
};
const CHIP_LABELS: Record<Capability, string> = {
  search: "Search",
  catalog: "Browse",
  metadata: "Track details",
  sources: "Playback",
  source_preparation: "Faster playback",
  playlists: "Playlists",
  artwork: "Covers",
  dashboard: "Dashboard",
  lyrics: "Lyrics",
  backing_tracks: "Backing tracks",
  featured: "Featured",
};
const NEW_ORDER: Capability[] = [
  "search",
  "sources",
  "playlists",
  "catalog",
  "metadata",
  "lyrics",
  "artwork",
  "source_preparation",
  "backing_tracks",
  "featured",
  "dashboard",
];

function phrases(name: string, has: (value: Capability) => boolean) {
  const found: string[] = [];
  if (has("search") && has("sources"))
    found.push(`finds songs and plays them from ${name}`);
  else if (has("sources")) found.push(`plays songs from ${name}`);
  else if (has("search")) found.push(`finds songs on ${name}`);
  if (has("catalog") && !has("search"))
    found.push(`lists albums and artists from ${name}`);
  if (has("playlists"))
    found.push(
      /playlists?$/i.test(name)
        ? `imports and follows ${name}`
        : `imports and follows ${name} playlists`,
    );
  if (has("metadata")) found.push("fills in track details");
  if (has("lyrics")) found.push("shows lyrics");
  if (has("artwork")) found.push("fetches album covers");
  if (has("source_preparation") && !has("sources"))
    found.push("gets songs ready to play sooner");
  if (has("backing_tracks")) found.push("offers backing tracks");
  if (has("featured")) found.push("features music on Discover");
  return found;
}

function sentence(items: string[]): string {
  const [first, ...rest] = items;
  const tail =
    rest.length === 0
      ? ""
      : `, and ${rest.length === 1 ? rest[0] : `${rest.slice(0, -1).join(", ")} and ${rest[rest.length - 1]}`}`;
  return `${first.charAt(0).toUpperCase()}${first.slice(1)}${tail}.`;
}

export function serviceSummary(
  name: string,
  capabilities: Capability[],
): string {
  const items = phrases(name, (value) => capabilities.includes(value));
  return items.length > 0
    ? sentence(items)
    : "Does not offer anything to listen to yet.";
}

export function newCapabilities(
  accepted: Capability[],
  proposed: Capability[],
): string[] {
  return NEW_ORDER.filter(
    (value) => proposed.includes(value) && !accepted.includes(value),
  ).map((value) => NEW_LABELS[value]);
}

export function capabilityChips(capabilities: Capability[]): string[] {
  return NEW_ORDER.filter((value) => capabilities.includes(value)).map(
    (value) => CHIP_LABELS[value],
  );
}

export function grantedAccess(scopes: string[]): string {
  return scopes
    .map((scope) =>
      (scope.split("/").filter(Boolean).at(-1) ?? scope)
        .replace(/[-_:.]+/g, " ")
        .trim(),
    )
    .filter(Boolean)
    .join(", ");
}

const WANTS: Record<Capability, string> = {
  search: "search for songs",
  catalog: "browse albums and artists",
  metadata: "fill in track details",
  sources: "play songs",
  source_preparation: "get songs ready sooner",
  playlists: "import playlists",
  artwork: "fetch album covers",
  dashboard: "show a dashboard",
  lyrics: "show lyrics",
  backing_tracks: "offer backing tracks",
  featured: "feature music on Discover",
};
const EFFECT: Partial<Record<Capability, (name: string) => string>> = {
  artwork: (name) =>
    `When an album has no artwork, Hito can also ask ${name} for it.`,
  search: (name) => `Search can include ${name} results when you ask.`,
  sources: (name) =>
    `Tracks with no file can play from ${name}, after local files and saved copies.`,
  playlists: (name) =>
    `You can follow or copy playlists from ${name} by pasting a link.`,
  metadata: (name) =>
    `Missing titles, artists and albums can be filled in from ${name}.`,
  lyrics: (name) => `Now playing can show lyrics from ${name}.`,
};

function addedCapabilities(accepted: Capability[], proposed: Capability[]) {
  return NEW_ORDER.filter(
    (value) => proposed.includes(value) && !accepted.includes(value),
  );
}

export function wantedActions(
  accepted: Capability[],
  proposed: Capability[],
): string {
  const items = addedCapabilities(accepted, proposed).map(
    (value) => WANTS[value],
  );
  if (items.length === 0) return "";
  if (items.length === 1) return items[0];
  return `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`;
}

export function acceptanceEffects(
  name: string,
  accepted: Capability[],
  proposed: Capability[],
): string[] {
  return addedCapabilities(accepted, proposed).map((value) =>
    (
      EFFECT[value] ??
      ((service: string) =>
        `Hito can also use ${service} for ${NEW_LABELS[value]}.`)
    )(name),
  );
}

export function newCapabilityChips(
  accepted: Capability[],
  proposed: Capability[],
): string[] {
  return newCapabilities(accepted, proposed).map(
    (label) => `${label.charAt(0).toUpperCase()}${label.slice(1)}`,
  );
}
