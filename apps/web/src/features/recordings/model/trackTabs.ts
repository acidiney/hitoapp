export const trackTabs = [
  "overview",
  "lyrics",
  "sources",
  "metadata",
  "history",
] as const;

export type TrackTab = (typeof trackTabs)[number];

export const trackAnchors = {
  "find-audio": "sources",
  "server-copy": "sources",
  "artist-match": "metadata",
  copies: "metadata",
} as const satisfies Record<string, TrackTab>;

export type TrackAnchor = keyof typeof trackAnchors;

export type TrackLocation = { tab: TrackTab; anchor: TrackAnchor | null };

export function isTrackTab(value: string | null): value is TrackTab {
  return trackTabs.some((tab) => tab === value);
}

export function isTrackAnchor(value: string): value is TrackAnchor {
  return Object.hasOwn(trackAnchors, value);
}

export function readTrackLocation(search: string, hash: string): TrackLocation {
  const params = new URLSearchParams(search);
  const fragment = hash.replace(/^#/, "");
  if (isTrackAnchor(fragment))
    return { tab: trackAnchors[fragment], anchor: fragment };
  if (params.get("review") === "artist")
    return { tab: "metadata", anchor: "artist-match" };
  const requested = params.get("tab");
  return { tab: isTrackTab(requested) ? requested : "overview", anchor: null };
}

export function trackTabHref(recording: string, tab: TrackTab): string {
  const base = `/recordings/${encodeURIComponent(recording)}`;
  return tab === "overview" ? base : `${base}?tab=${tab}`;
}

export function findAudioHref(recording: string): string {
  return `${trackTabHref(recording, "sources")}#find-audio`;
}

export function trackTabForShortcut(key: string): TrackTab | null {
  const index = Number(key) - 1;
  return Number.isInteger(index) ? (trackTabs[index] ?? null) : null;
}

export function editableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return (
    target.isContentEditable ||
    ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName) ||
    target.closest("dialog[open]") !== null ||
    target.getAttribute("role") === "combobox"
  );
}
