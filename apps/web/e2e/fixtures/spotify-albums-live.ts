import { isDeepStrictEqual } from "node:util";
import type { APIRequestContext } from "@playwright/test";
import type { components } from "../../src/generated";

type Schema = components["schemas"];
type ObjectValue = Record<string, unknown>;
type LiveEntry = {
  occurrence_id: string | null;
  kind: Schema["ImportedPlaylistEntryKind"];
  provider_recording_id: string | null;
  title: string | null;
  artist: string | null;
  duration_ms: number;
};

export function liveCheck(condition: unknown): asserts condition {
  if (!condition) throw new Error("Live album boundary verification failed");
}

function object(value: unknown): ObjectValue {
  liveCheck(
    value !== null && typeof value === "object" && !Array.isArray(value),
  );
  return value as ObjectValue;
}

function array(value: unknown): unknown[] {
  liveCheck(Array.isArray(value));
  return value;
}

function text(value: unknown, maximum: number): string {
  liveCheck(
    typeof value === "string" &&
      value.trim().length > 0 &&
      Buffer.byteLength(value) <= maximum &&
      !/\p{Cc}/u.test(value),
  );
  return value;
}

function optionalText(value: unknown, maximum: number): string | null {
  return value == null ? null : text(value, maximum);
}

function number(value: unknown, maximum: number, minimum = 0): number {
  liveCheck(
    typeof value === "number" &&
      Number.isSafeInteger(value) &&
      value >= minimum &&
      value <= maximum,
  );
  return value;
}

export function normalizedLiveRecording(
  value: unknown,
): Schema["ProviderRecording"] {
  const recording = object(value);
  const id = text(recording.id, 256);
  liveCheck(/^[A-Za-z0-9]{22}$/.test(id));
  const album = recording.release == null ? null : object(recording.release);
  const release =
    album === null
      ? null
      : {
          id: text(album.id, 256),
          title: text(album.title, 2048),
          artist: optionalText(album.artist, 2048),
        };
  liveCheck(release === null || /^[A-Za-z0-9]{22}$/.test(release.id));
  return {
    id,
    title: text(recording.title, 2048),
    artist: text(recording.artist, 2048),
    duration_ms:
      recording.duration_ms == null
        ? null
        : number(recording.duration_ms, Number.MAX_SAFE_INTEGER),
    release,
    disc_number:
      recording.disc_number == null
        ? null
        : number(recording.disc_number, 4_294_967_295, 1),
    track_number:
      recording.track_number == null
        ? null
        : number(recording.track_number, 4_294_967_295, 1),
    revision: optionalText(recording.revision, 256),
  };
}

export function spotifyAlbumsLiveConfiguration() {
  const supplied = process.env.HITO_SPOTIFY_TEST_PLAYLIST;
  liveCheck(supplied && Buffer.byteLength(supplied) <= 2048);
  const reference = new URL(supplied);
  liveCheck(
    reference.protocol === "https:" &&
      reference.hostname === "open.spotify.com" &&
      !reference.port &&
      !reference.username &&
      !reference.password &&
      !reference.hash &&
      /^\/playlist\/[A-Za-z0-9]{22}$/.test(reference.pathname),
  );
  const pluginAddress =
    process.env.HITO_SPOTIFY_PLUGIN_URL ?? "http://plugin-spotify:8092/";
  liveCheck(pluginAddress === "http://plugin-spotify:8092/");
  return {
    reference: supplied,
    playlistId: reference.pathname.slice(10),
    pluginAddress,
  };
}

export async function readSpotifyAlbumsLive(
  request: APIRequestContext,
  playlistId: string,
  reference: string,
  mode: "current" | "legacy" = "current",
) {
  let remaining = 16 * 1024 * 1024;
  let requests = 0;
  const deadline = Date.now() + 60_000;
  async function json(path: string, maximum: number, data?: unknown) {
    liveCheck(++requests <= 103 && Date.now() < deadline);
    const options = {
      timeout: Math.min(15_000, deadline - Date.now()),
      maxRedirects: 0,
      maxRetries: 0,
    };
    const response =
      data === undefined
        ? await request.get(path, options)
        : await request.post(path, { ...options, data });
    try {
      liveCheck(response.status() === 200);
      const headers = response.headers();
      liveCheck(
        headers["content-type"]?.split(";")[0].trim() === "application/json",
      );
      if (headers["content-length"] !== undefined)
        liveCheck(
          number(Number(headers["content-length"]), maximum) <= remaining,
        );
      const body = await response.body();
      liveCheck(
        body.length <= maximum &&
          body.length <= remaining &&
          Date.now() <= deadline,
      );
      remaining -= body.length;
      return object(JSON.parse(body.toString()) as unknown);
    } finally {
      await response.dispose();
    }
  }
  const manifest = await json("v1/manifest", 32_768);
  const capabilities = array(manifest.capabilities);
  liveCheck(
    manifest.id === "org.hito.spotify" &&
      manifest.protocol_version === 1 &&
      capabilities.includes("playlists") &&
      !capabilities.some((item) =>
        ["sources", "source_preparation", "search"].includes(String(item)),
      ) &&
      manifest.authorization == null,
  );
  const manifestName = text(manifest.name, 256);
  function detail(value: ObjectValue) {
    liveCheck(text(value.id, 256) === playlistId);
    return {
      id: playlistId,
      name: text(value.name, 256),
      snapshot: text(value.snapshot, 256),
      total_count: number(value.total_count, 10_000),
      attribution: { label: text(object(value.attribution).label, 2048) },
    };
  }
  const initial = detail(
    await json("v1/playlists/detail", 32_768, { kind: "reference", reference }),
  );
  const entries: LiveEntry[] = [];
  const recordings = new Map<string, Schema["ProviderRecording"]>();
  const occurrences = new Set<string>();
  const cursors = new Set<string>();
  let cursor: string | null = null;
  let pages = 0;
  do {
    liveCheck(++pages <= 100);
    const page = await json("v1/playlists/items", 1024 * 1024, {
      playlist_id: initial.id,
      snapshot: initial.snapshot,
      cursor,
      limit: 100,
    });
    liveCheck(
      page.playlist_id === initial.id && page.snapshot === initial.snapshot,
    );
    const items = array(page.items);
    liveCheck(
      items.length <= 100 &&
        entries.length + items.length <= initial.total_count,
    );
    for (const value of items) {
      const source = object(value);
      const occurrence = optionalText(source.occurrence_id, 256);
      liveCheck(occurrence === null || !occurrences.has(occurrence));
      if (occurrence !== null) occurrences.add(occurrence);
      const item = object(source.item);
      if (item.kind === "recording") {
        const recording = normalizedLiveRecording(item.recording);
        const previous = recordings.get(recording.id);
        liveCheck(
          previous === undefined || isDeepStrictEqual(previous, recording),
        );
        recordings.set(recording.id, recording);
        entries.push({
          occurrence_id: occurrence,
          kind: "recording",
          provider_recording_id: recording.id,
          title: recording.title,
          artist: recording.artist,
          duration_ms: recording.duration_ms ?? 0,
        });
      } else {
        liveCheck(item.kind === "unknown" || item.kind === "unavailable");
        entries.push({
          occurrence_id: occurrence,
          kind: item.kind,
          provider_recording_id:
            item.kind === "unavailable"
              ? optionalText(item.provider_recording_id, 256)
              : null,
          title:
            item.kind === "unavailable" ? optionalText(item.title, 2048) : null,
          artist:
            item.kind === "unavailable"
              ? optionalText(item.artist, 2048)
              : null,
          duration_ms: 0,
        });
      }
    }
    cursor = optionalText(page.next_cursor, 2048);
    if (cursor !== null) {
      liveCheck(
        items.length > 0 &&
          entries.length < initial.total_count &&
          !cursors.has(cursor),
      );
      cursors.add(cursor);
    }
  } while (cursor !== null);
  liveCheck(entries.length === initial.total_count);
  const final = detail(
    await json("v1/playlists/detail", 32_768, {
      kind: "canonical",
      playlist_id: initial.id,
    }),
  );
  liveCheck(isDeepStrictEqual(initial, final));
  liveCheck(
    mode === "legacy"
      ? recordings.size > 0 &&
          [...recordings.values()].every(
            (recording) => recording.release == null,
          )
      : [...recordings.values()].some((recording) => recording.release != null),
  );
  return {
    detail: initial,
    entries,
    recordings,
    manifestName,
    pages,
    requests,
  };
}

export function verifySpotifyAlbumsPublication(
  expected: Awaited<ReturnType<typeof readSpotifyAlbumsLive>>,
  playlist: Schema["PlaylistDetail"],
  publication: Schema["ImportedPlaylistSnapshot"],
  connection: Schema["PluginInstallation"],
  service: Schema["PluginService"],
) {
  liveCheck(
    playlist.playlist.kind === "local" &&
      playlist.playlist.name === expected.detail.name &&
      publication.playlist_id === playlist.playlist.id &&
      publication.name === expected.detail.name &&
      isDeepStrictEqual(publication.origin, playlist.origin) &&
      playlist.origin?.provider_playlist_id === expected.detail.id &&
      playlist.origin.snapshot === expected.detail.snapshot &&
      playlist.origin.provider === connection.manifest.id &&
      playlist.origin.installation_id === connection.id &&
      playlist.origin.installation_revision === connection.revision &&
      playlist.origin.service_id === service.id &&
      playlist.origin.service_revision === service.revision &&
      isDeepStrictEqual(
        playlist.origin.attribution,
        expected.detail.attribution,
      ),
  );
  liveCheck(publication.recordings.length === expected.recordings.size);
  const recordings = new Map<string, Schema["ImportedPlaylistRecording"]>();
  const releases = new Map<string, string>();
  for (const item of publication.recordings) {
    const metadata = normalizedLiveRecording(item.metadata);
    liveCheck(
      !recordings.has(metadata.id) &&
        isDeepStrictEqual(expected.recordings.get(metadata.id), metadata),
    );
    liveCheck(
      /^[0-9a-f-]{36}$/.test(item.recording_id) &&
        /^[0-9a-f-]{36}$/.test(item.library_id),
    );
    recordings.set(metadata.id, item);
    if (metadata.release) {
      liveCheck(
        typeof item.release_id === "string" &&
          /^[0-9a-f-]{36}$/.test(item.release_id),
      );
      const previous = releases.get(metadata.release.id);
      liveCheck(previous === undefined || previous === item.release_id);
      releases.set(metadata.release.id, item.release_id);
    } else liveCheck(item.release_id == null);
  }
  liveCheck(new Set(releases.values()).size === releases.size);
  liveCheck(
    playlist.entries.length === expected.entries.length &&
      publication.entries.length === expected.entries.length,
  );
  liveCheck(
    new Set(playlist.entries.map((entry) => entry.entry_id)).size ===
      expected.entries.length,
  );
  for (const [position, source] of expected.entries.entries()) {
    const entry = playlist.entries[position];
    const published = publication.entries[position];
    const recording =
      source.provider_recording_id === null
        ? undefined
        : recordings.get(source.provider_recording_id);
    liveCheck(
      entry.position === position &&
        published.position === position &&
        entry.entry_id === published.entry_id &&
        /^[0-9a-f-]{36}$/.test(entry.entry_id) &&
        !entry.available &&
        !entry.liked &&
        isDeepStrictEqual(entry.origin, published.origin) &&
        entry.origin?.kind === source.kind &&
        (entry.origin.occurrence_id ?? null) === source.occurrence_id &&
        (entry.origin.provider_recording_id ?? null) ===
          source.provider_recording_id &&
        (entry.recording_id ?? null) ===
          (source.kind === "recording" ? recording?.recording_id : null) &&
        (published.recording_id ?? null) === (entry.recording_id ?? null),
    );
    if (source.kind === "recording") {
      liveCheck(
        recording &&
          entry.title === source.title &&
          entry.artist === source.artist &&
          entry.duration_ms === source.duration_ms,
      );
      liveCheck(
        entry.inherited_title === source.title &&
          entry.inherited_artist === source.artist,
      );
    } else {
      liveCheck(source.title === null || entry.title === source.title);
      liveCheck(source.artist === null || entry.artist === source.artist);
    }
  }
  return { recordings, releases };
}
