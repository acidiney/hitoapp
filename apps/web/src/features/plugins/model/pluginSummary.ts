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
  if (has("playlists")) found.push(`imports and follows ${name} playlists`);
  if (has("metadata")) found.push("fills in track details");
  if (has("lyrics")) found.push("shows lyrics");
  if (has("artwork")) found.push("fetches album covers");
  if (has("source_preparation") && !has("sources"))
    found.push("gets songs ready to play sooner");
  if (has("backing_tracks")) found.push("offers backing tracks");
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
