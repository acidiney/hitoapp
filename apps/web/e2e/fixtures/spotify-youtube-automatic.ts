import { randomUUID } from "node:crypto";
import { expect, type Page } from "@playwright/test";
import type { components } from "../../src/generated";
import {
  readSpotifyYouTube as read,
  retainedImportedPlaylist,
  uuid,
} from "./spotify-youtube-collection";

type Schema = components["schemas"];

export async function automaticLiveCollection(
  page: Page,
  recording: string,
  playlist: string,
) {
  const [account, metadata, detail, playlists, catalog, activity] =
    await Promise.all([
      read<Schema["User"]>(page, "/api/v1/me"),
      read<Schema["RecordingMetadata"]>(
        page,
        `/api/v1/recordings/${recording}/metadata`,
      ),
      read<Schema["PlaylistDetail"]>(page, `/api/v1/playlists/${playlist}`),
      read<Schema["Playlist"][]>(page, "/api/v1/playlists"),
      read<Schema["RecordingPage"]>(page, "/api/v1/recordings?limit=1"),
      read<Schema["Activity"]>(page, "/api/v1/activity"),
    ]);
  return { account, metadata, detail, playlists, catalog, activity };
}

export async function seedAutomaticLiveState(
  page: Page,
  origin: string,
  recording: string,
  playlist: string,
  title: string,
  artist: string,
) {
  const response = await page.request.post("/api/v1/sync", {
    headers: { "x-hito-request": "1", origin },
    data: {
      device_id: "spotify-youtube-automatic-live",
      operations: [
        {
          id: randomUUID(),
          action: {
            kind: "recording_title_set",
            recording_id: recording,
            title,
          },
        },
        {
          id: randomUUID(),
          action: {
            kind: "recording_artist_set",
            recording_id: recording,
            artist,
          },
        },
        {
          id: randomUUID(),
          action: { kind: "like_set", recording_id: recording, liked: true },
        },
        {
          id: randomUUID(),
          action: {
            kind: "progress_set",
            recording_id: recording,
            position_ms: 431,
            speed: 1.25,
          },
        },
      ],
    } satisfies Schema["SyncBatch"],
  });
  expect(response.status()).toBe(200);
  const before = await automaticLiveCollection(page, recording, playlist);
  expect(
    uuid.test(before.account.id) &&
      before.metadata.title.personal === title &&
      before.metadata.artist.personal === artist &&
      before.detail.entries
        .filter((entry) => entry.recording_id === recording)
        .every((entry) => entry.liked && !entry.available) &&
      before.activity.history.some(
        (entry) =>
          entry.id === recording &&
          entry.liked &&
          entry.position_ms === 431 &&
          entry.speed === 1.25,
      ),
  ).toBe(true);
  return before;
}

export async function assertAutomaticLiveCollection(
  page: Page,
  recording: string,
  playlist: string,
  youtube: Schema["PluginInstallation"],
  before: Awaited<ReturnType<typeof automaticLiveCollection>>,
  playedSource: string,
  resolution: Schema["SourceResolution"],
  observedSeconds: number,
) {
  const after = await automaticLiveCollection(page, recording, playlist);
  expect(
    JSON.stringify(after.account) === JSON.stringify(before.account) &&
      after.metadata.recording_id === recording &&
      JSON.stringify(after.metadata.title) ===
        JSON.stringify(before.metadata.title) &&
      JSON.stringify(after.metadata.artist) ===
        JSON.stringify(before.metadata.artist) &&
      JSON.stringify(after.metadata.selected_observation) ===
        JSON.stringify(before.metadata.selected_observation) &&
      JSON.stringify(retainedImportedPlaylist(after.detail, recording)) ===
        JSON.stringify(retainedImportedPlaylist(before.detail, recording)) &&
      JSON.stringify(after.playlists) === JSON.stringify(before.playlists) &&
      after.catalog.total === before.catalog.total &&
      after.detail.entries
        .filter((entry) => entry.recording_id === recording)
        .every((entry) => entry.available && entry.liked),
  ).toBe(true);
  const added = after.metadata.observations.items.filter(
    (item) =>
      !before.metadata.observations.items.some((old) => old.id === item.id),
  );
  expect(
    added.length === 1 &&
      added[0].origin === "provider_response" &&
      added[0].provider === youtube.manifest.id &&
      added[0].plugin_installation_id === youtube.id &&
      /^[A-Za-z0-9_-]{11}$/.test(added[0].provider_recording_id ?? "") &&
      before.metadata.observations.items.every((old) =>
        after.metadata.observations.items.some(
          (item) => JSON.stringify(item) === JSON.stringify(old),
        ),
      ),
  ).toBe(true);
  const sources = await read<Schema["Source"][]>(
    page,
    `/api/v1/recordings/${recording}/sources`,
  );
  expect(
    sources.length === 1 &&
      sources[0].id === playedSource &&
      sources[0].recording_id === recording &&
      sources[0].provider === "plugin" &&
      sources[0].ready &&
      uuid.test(sources[0].id) &&
      resolution.recording_id === recording &&
      resolution.outcome === "ready" &&
      uuid.test(resolution.resolution_id) &&
      resolution.sources.some(
        (source) => source.id === playedSource && source.ready,
      ),
  ).toBe(true);
  const choice = await read<Schema["SourceChoice"]>(
    page,
    `/api/v1/recordings/${recording}/source-choice`,
  );
  expect(choice.mode === "automatic" && choice.revision === 0).toBe(true);
  await expect
    .poll(
      async () => {
        const activity = await read<Schema["Activity"]>(
          page,
          "/api/v1/activity",
        );
        return activity.history.some(
          (item) =>
            item.id === recording &&
            item.liked &&
            item.title === before.metadata.title.value &&
            item.artist === before.metadata.artist.value &&
            item.position_ms >= 5000 &&
            Math.abs(item.position_ms - observedSeconds * 1000) <= 2000 &&
            item.speed > 0,
        );
      },
      { timeout: 10_000 },
    )
    .toBe(true);
  for (const path of [
    `/api/v1/recordings/${recording}/download`,
    `/api/v1/playlists/${playlist}/download`,
  ])
    expect((await page.request.get(path)).status()).toBe(404);
  return { after, added };
}
