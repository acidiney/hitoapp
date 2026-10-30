import { randomUUID } from "node:crypto";
import { expect, type Page } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";
import { SpotifyYouTubeLiveFailure } from "./spotify-youtube-live";

export const uuid =
  /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/;

export async function readSpotifyYouTube<T>(
  page: Page,
  path: string,
): Promise<T> {
  const response = await page.request.get(path);
  if (response.status() !== 200)
    throw new SpotifyYouTubeLiveFailure(
      `Collection inspection returned HTTP ${response.status()}`,
    );
  return response.json();
}

export function retainedImportedPlaylist(
  detail: Schema<"PlaylistDetail">,
  recording: string,
) {
  return {
    ...detail,
    entries: detail.entries.map((entry) =>
      entry.recording_id === recording ? { ...entry, available: false } : entry,
    ),
  };
}

export async function inspectCopiedPlaylist(
  page: Page,
  receipt: Schema<"PlaylistImportReceipt">,
  spotify: Schema<"PluginInstallation">,
) {
  const copied = await readSpotifyYouTube<Schema<"PlaylistDetail">>(
    page,
    `/api/v1/playlists/${receipt.playlist_id}`,
  );
  expect(
    copied.playlist.id === receipt.playlist_id &&
      copied.playlist.kind === "local",
  ).toBe(true);
  expect(
    copied.origin?.provider === spotify.manifest.id &&
      copied.origin?.installation_id === spotify.id &&
      copied.origin?.attribution.label === "Spotify",
  ).toBe(true);
  expect(copied.entries.length).toBeGreaterThan(1);
  expect(copied.entries.length).toBeLessThanOrEqual(500);
  expect(copied.entries.every((entry) => !entry.available)).toBe(true);
  expect(new Set(copied.entries.map((entry) => entry.entry_id)).size).toBe(
    copied.entries.length,
  );
  const chosen = copied.entries.find(
    (entry) =>
      entry.recording_id &&
      entry.origin?.kind === "recording" &&
      entry.duration_ms > 10_000 &&
      entry.duration_ms <= 600_000,
  );
  if (!chosen?.recording_id)
    throw new SpotifyYouTubeLiveFailure(
      "The copied playlist has no bounded saved track for explicit review",
    );
  const recording = chosen.recording_id;
  expect(
    uuid.test(recording) && recording !== chosen.origin?.provider_recording_id,
  ).toBe(true);
  return { copied, recording };
}

export async function inspectImportedRecording(
  page: Page,
  recording: string,
  spotify: Schema<"PluginInstallation">,
) {
  const originalMetadata = await readSpotifyYouTube<
    Schema<"RecordingMetadata">
  >(page, `/api/v1/recordings/${recording}/metadata`);
  const original = originalMetadata.selected_observation;
  if (
    !original ||
    original.origin !== "provider_response" ||
    original.provider !== spotify.manifest.id ||
    original.plugin_installation_id !== spotify.id
  )
    throw new SpotifyYouTubeLiveFailure(
      "The saved track must retain its Spotify metadata owner",
    );
  expect(
    (
      await readSpotifyYouTube<Schema<"Source">[]>(
        page,
        `/api/v1/recordings/${recording}/sources`,
      )
    ).length,
  ).toBe(0);
  return original;
}

export async function editPrivateDetailsAndCaptureBaseline(
  page: Page,
  {
    receipt,
    recording,
    initialPlaylists,
    personalTitle,
    personalArtist,
    headers,
  }: {
    receipt: Schema<"PlaylistImportReceipt">;
    recording: string;
    initialPlaylists: Schema<"Playlist">[];
    personalTitle: string;
    personalArtist: string;
    headers: Record<string, string>;
  },
) {
  const edited = await page.request.post("/api/v1/sync", {
    headers,
    data: {
      device_id: "spotify-youtube-live",
      operations: [
        {
          id: randomUUID(),
          action: {
            kind: "recording_title_set",
            recording_id: recording,
            title: personalTitle,
          },
        },
        {
          id: randomUUID(),
          action: {
            kind: "recording_artist_set",
            recording_id: recording,
            artist: personalArtist,
          },
        },
      ],
    } satisfies Schema<"SyncBatch">,
  });
  if (edited.status() !== 200)
    throw new SpotifyYouTubeLiveFailure(
      `Owned private edits returned HTTP ${edited.status()}`,
    );
  const beforeMetadata = await readSpotifyYouTube<Schema<"RecordingMetadata">>(
    page,
    `/api/v1/recordings/${recording}/metadata`,
  );
  const beforePlaylist = await readSpotifyYouTube<Schema<"PlaylistDetail">>(
    page,
    `/api/v1/playlists/${receipt.playlist_id}`,
  );
  const beforeLists = await readSpotifyYouTube<Schema<"Playlist">[]>(
    page,
    "/api/v1/playlists",
  );
  const beforeCatalog = await readSpotifyYouTube<Schema<"RecordingPage">>(
    page,
    "/api/v1/recordings?limit=1",
  );
  expect(beforeLists.length).toBe(initialPlaylists.length + 1);
  expect(
    beforeMetadata.title.personal === personalTitle &&
      beforeMetadata.artist.personal === personalArtist,
  ).toBe(true);
  return { beforeMetadata, beforePlaylist, beforeLists, beforeCatalog };
}

export type AttachmentBaseline = Awaited<
  ReturnType<typeof editPrivateDetailsAndCaptureBaseline>
>;

export async function assertReviewPreservesPlaylist(
  page: Page,
  receipt: Schema<"PlaylistImportReceipt">,
  beforePlaylist: Schema<"PlaylistDetail">,
) {
  expect(
    JSON.stringify(
      await readSpotifyYouTube<Schema<"PlaylistDetail">>(
        page,
        `/api/v1/playlists/${receipt.playlist_id}`,
      ),
    ) === JSON.stringify(beforePlaylist),
  ).toBe(true);
}

export async function assertAttachedCollection(
  page: Page,
  {
    receipt,
    recording,
    baseline,
    youtube,
    selection,
  }: {
    receipt: Schema<"PlaylistImportReceipt">;
    recording: string;
    baseline: AttachmentBaseline;
    youtube: Schema<"PluginInstallation">;
    selection: { candidate: Schema<"ProviderRecording"> };
  },
) {
  const { beforeMetadata, beforePlaylist, beforeLists, beforeCatalog } =
    baseline;
  const afterMetadata = await readSpotifyYouTube<Schema<"RecordingMetadata">>(
    page,
    `/api/v1/recordings/${recording}/metadata`,
  );
  expect(
    afterMetadata.recording_id === recording &&
      JSON.stringify(afterMetadata.title) ===
        JSON.stringify(beforeMetadata.title) &&
      JSON.stringify(afterMetadata.artist) ===
        JSON.stringify(beforeMetadata.artist) &&
      JSON.stringify(afterMetadata.selected_observation) ===
        JSON.stringify(beforeMetadata.selected_observation),
  ).toBe(true);
  expect(
    afterMetadata.observations.items.some(
      (item) =>
        item.provider === youtube.manifest.id &&
        item.plugin_installation_id === youtube.id &&
        item.provider_recording_id === selection.candidate.id,
    ),
  ).toBe(true);
  const afterPlaylist = await readSpotifyYouTube<Schema<"PlaylistDetail">>(
    page,
    `/api/v1/playlists/${receipt.playlist_id}`,
  );
  expect(
    JSON.stringify(retainedImportedPlaylist(afterPlaylist, recording)) ===
      JSON.stringify(retainedImportedPlaylist(beforePlaylist, recording)),
  ).toBe(true);
  expect(
    afterPlaylist.entries
      .filter((entry) => entry.recording_id === recording)
      .every((entry) => entry.available),
  ).toBe(true);
  expect(
    afterPlaylist.entries
      .filter((entry) => entry.recording_id !== recording)
      .every((entry) => !entry.available),
  ).toBe(true);
  expect(
    JSON.stringify(
      await readSpotifyYouTube<Schema<"Playlist">[]>(page, "/api/v1/playlists"),
    ) === JSON.stringify(beforeLists),
  ).toBe(true);
  expect(
    (
      await readSpotifyYouTube<Schema<"RecordingPage">>(
        page,
        "/api/v1/recordings?limit=1",
      )
    ).total,
  ).toBe(beforeCatalog.total);
  const sources = await readSpotifyYouTube<Schema<"Source">[]>(
    page,
    `/api/v1/recordings/${recording}/sources`,
  );
  expect(
    sources.length > 0 &&
      sources.every(
        (source) =>
          uuid.test(source.id) &&
          source.ready &&
          source.recording_id === recording &&
          source.provider === "plugin",
      ),
  ).toBe(true);
  return { afterPlaylist, sources };
}

export async function assertPlaybackPreservesCollection(
  page: Page,
  receipt: Schema<"PlaylistImportReceipt">,
  recording: string,
  afterPlaylist: Schema<"PlaylistDetail">,
) {
  const preserved = await readSpotifyYouTube<Schema<"PlaylistDetail">>(
    page,
    `/api/v1/playlists/${receipt.playlist_id}`,
  );
  expect(JSON.stringify(preserved) === JSON.stringify(afterPlaylist)).toBe(
    true,
  );
  const history = await readSpotifyYouTube<Schema<"Activity">>(
    page,
    "/api/v1/activity",
  );
  expect(history.history.some((item) => item.id === recording)).toBe(true);
}
