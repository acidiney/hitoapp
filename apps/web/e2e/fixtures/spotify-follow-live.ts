import type { APIRequestContext } from "@playwright/test";
import type { components } from "../../src/generated";

type Schema = components["schemas"];
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

export class SpotifyFollowLiveFailure extends Error {
  constructor(readonly code: number) {
    super(`Spotify Follow live boundary failed (${code})`);
  }
}

function check(condition: unknown, code = 3): asserts condition {
  if (!condition) throw new SpotifyFollowLiveFailure(code);
}

function record(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function object(value: unknown) {
  check(record(value));
  return value;
}

function array(value: unknown): unknown[] {
  check(Array.isArray(value));
  return value;
}

function text(value: unknown): string {
  check(typeof value === "string" && value.length > 0);
  return value;
}

export function spotifyFollowLiveConfiguration() {
  try {
    const reference = new URL(process.env.HITO_SPOTIFY_TEST_PLAYLIST ?? "");
    const service = new URL(process.env.HITO_SPOTIFY_PLUGIN_URL ?? "");
    check(
      reference.protocol === "https:" &&
        reference.hostname === "open.spotify.com" &&
        !reference.port &&
        !reference.username &&
        !reference.password &&
        !reference.hash &&
        /^\/playlist\/[A-Za-z0-9]{22}$/.test(reference.pathname),
      1,
    );
    check(
      ["http:", "https:"].includes(service.protocol) &&
        !service.username &&
        !service.password &&
        !service.search &&
        !service.hash,
      1,
    );
    if (!service.pathname.endsWith("/")) service.pathname += "/";
    return { reference: reference.href, service: service.href };
  } catch {
    throw new SpotifyFollowLiveFailure(1);
  }
}

async function providerJSON(
  request: APIRequestContext,
  service: string,
  path: string,
  data: unknown,
) {
  try {
    const response = await request.post(new URL(path, service).href, { data });
    check(response.status() === 200, 2);
    const body: unknown = await response.json();
    return object(body);
  } catch (error) {
    if (error instanceof SpotifyFollowLiveFailure) throw error;
    throw new SpotifyFollowLiveFailure(2);
  }
}

function verifyEntry(
  entry: Schema["PlaylistEntry"],
  source: Record<string, unknown>,
  position: number,
  recordings: Map<string, string>,
) {
  const item = object(source.item);
  check(entry.position === position && !entry.available);
  check(entry.origin?.kind === item.kind);
  check(
    (entry.origin?.occurrence_id ?? null) === (source.occurrence_id ?? null),
  );
  if (item.kind === "recording") {
    const metadata = object(item.recording);
    const provider = text(metadata.id);
    check(entry.origin?.provider_recording_id === provider);
    check(
      entry.title === text(metadata.title) &&
        entry.artist === text(metadata.artist),
    );
    check(entry.duration_ms === (metadata.duration_ms ?? 0));
    check(
      typeof entry.recording_id === "string" && uuid.test(entry.recording_id),
    );
    const previous = recordings.get(provider);
    check(previous === undefined || previous === entry.recording_id);
    recordings.set(provider, entry.recording_id);
    return previous !== undefined ? 1 : 0;
  }
  check(item.kind === "unavailable" || item.kind === "unknown");
  check(entry.recording_id == null);
  check(
    (entry.origin?.provider_recording_id ?? null) ===
      (item.provider_recording_id ?? null),
  );
  if (item.kind === "unavailable") {
    if (item.title != null) check(entry.title === text(item.title));
    if (item.artist != null) check(entry.artist === text(item.artist));
  }
  return 0;
}

export async function verifySpotifyFollowSnapshot(
  request: APIRequestContext,
  service: string,
  accepted: Schema["PlaylistDetail"],
) {
  const origin = accepted.origin;
  check(origin && accepted.playlist.kind === "followed");
  const detail = await providerJSON(request, service, "v1/playlists/detail", {
    kind: "canonical",
    playlist_id: origin.provider_playlist_id,
  });
  check(detail.id === origin.provider_playlist_id);
  check(detail.snapshot === origin.snapshot, 4);
  check(detail.name === accepted.playlist.name);
  check(object(detail.attribution).label === origin.attribution.label);
  const total = detail.total_count;
  check(
    typeof total === "number" &&
      Number.isSafeInteger(total) &&
      total >= 0 &&
      total <= 10_000,
  );
  check(total === accepted.entries.length);
  check(
    new Set(accepted.entries.map((entry) => entry.entry_id)).size === total,
  );
  check(accepted.entries.every((entry) => uuid.test(entry.entry_id)));
  const cursors = new Set<string>();
  const recordings = new Map<string, string>();
  let cursor: string | null = null;
  let pages = 0;
  let entries = 0;
  let duplicates = 0;
  do {
    const page = await providerJSON(request, service, "v1/playlists/items", {
      playlist_id: origin.provider_playlist_id,
      snapshot: origin.snapshot,
      cursor,
      limit: 100,
    });
    check(
      page.playlist_id === origin.provider_playlist_id &&
        page.snapshot === origin.snapshot,
    );
    const items = array(page.items);
    check(items.length <= 100 && entries + items.length <= total);
    pages++;
    check(pages <= 100);
    for (const source of items) {
      duplicates += verifyEntry(
        accepted.entries[entries],
        object(source),
        entries,
        recordings,
      );
      entries++;
    }
    cursor = page.next_cursor == null ? null : text(page.next_cursor);
    if (cursor !== null) {
      check(cursor.length <= 2048 && !cursors.has(cursor));
      cursors.add(cursor);
    }
  } while (cursor !== null);
  check(entries === total);
  return { entries, pages, duplicates, recordings };
}
