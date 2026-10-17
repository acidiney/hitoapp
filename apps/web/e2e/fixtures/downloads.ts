import { createHash, randomUUID } from "node:crypto";
import { expect, type Page } from "@playwright/test";
import type { components } from "../../src/generated";
import { providerFixture } from "./provider";
import { setUpSpotifyYouTubeLiveAccount as setUpDisposableDownloadAccount } from "./spotify-youtube-live";

type Schema = components["schemas"];
export type DownloadProvider = Awaited<ReturnType<typeof providerFixture>>;

export async function read<T>(page: Page, path: string): Promise<T> {
  const response = await page.request.get(path);
  expect(response.status()).toBe(200);
  return response.json();
}

async function originalAudioDigest(page: Page, source: string) {
  const response = await page.request.get(`/api/v1/sources/${source}/audio`);
  expect(response.status()).toBe(200);
  const bytes = await response.body();
  expect(bytes.length).toBeGreaterThan(0);
  return createHash("sha256").update(bytes).digest("hex");
}

export async function prepareDownloadLibrary(
  page: Page,
  baseURL: string,
  provider: DownloadProvider,
) {
  await setUpDisposableDownloadAccount(page);
  await page
    .getByRole("navigation", { name: "Main navigation" })
    .getByRole("link", { name: "Settings", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Add music folder", exact: true })
    .click();
  const picker = page.getByRole("dialog", {
    name: "Add a music folder",
    exact: true,
  });
  await picker.getByRole("button", { name: "Open Music", exact: true }).click();
  await picker
    .getByRole("button", { name: "Use this folder", exact: true })
    .click();
  await expect(picker).not.toBeVisible();
  await expect(
    page.locator(".music-folder-row").filter({ hasText: "1 file read" }),
  ).toHaveCount(1, { timeout: 20_000 });
  const originals = await read<Schema["RecordingPage"]>(
    page,
    "/api/v1/recordings",
  );
  expect(originals.items.length).toBe(1);
  const original = originals.items[0];
  const originalSources = await read<Schema["Source"][]>(
    page,
    `/api/v1/recordings/${original.id}/sources`,
  );
  expect(originalSources.length).toBe(1);
  expect(
    originalSources[0].provider === "local" && originalSources[0].ready,
  ).toBe(true);
  const digest = await originalAudioDigest(page, originalSources[0].id);
  const headers = { "x-hito-request": "1", origin: new URL(baseURL).origin };
  const offered = await page.request.post("/api/v1/plugins/services", {
    headers,
    data: {
      name: "Download fixture",
      base_url: provider.address,
    } satisfies Schema["OfferPluginService"],
  });
  expect(offered.status()).toBe(201);
  const service: Schema["PluginService"] = await offered.json();
  const installed = await page.request.post("/api/v1/plugins", {
    headers,
    data: {
      service_id: service.id,
      service_revision: service.revision,
      bearer_token: provider.token,
    } satisfies Schema["InstallPlugin"],
  });
  expect(installed.status()).toBe(201);
  const installation: Schema["PluginInstallation"] = await installed.json();
  const search = await page.request.post(
    `/api/v1/plugins/${installation.id}/search`,
    {
      headers,
      data: {
        installation_revision: installation.revision,
        query: "Download",
        limit: 10,
      } satisfies Schema["ProviderSearch"],
    },
  );
  expect(search.status()).toBe(200);
  const found: Schema["SearchPage"] = await search.json();
  expect(found.items.some((item) => item.id === provider.recording.id)).toBe(
    true,
  );
  const adopted = await page.request.post(
    `/api/v1/plugins/${installation.id}/recordings`,
    {
      headers,
      data: {
        installation_revision: installation.revision,
        provider_recording_id: provider.recording.id,
      } satisfies Schema["AdoptProviderRecording"],
    },
  );
  expect(adopted.status()).toBe(200);
  const recording: Schema["Recording"] = await adopted.json();
  const playlist = randomUUID();
  const personalTitle = "Saved download personal title";
  const actions: Schema["SyncBatch"] = {
    device_id: "download-browser-fixture",
    operations: [
      {
        id: randomUUID(),
        action: {
          kind: "recording_title_set",
          recording_id: recording.id,
          title: personalTitle,
        },
      },
      {
        id: randomUUID(),
        action: {
          kind: "recording_artist_set",
          recording_id: recording.id,
          artist: "Saved download personal artist",
        },
      },
      {
        id: randomUUID(),
        action: {
          kind: "playlist_create",
          playlist_id: playlist,
          name: "Preserved download selection",
        },
      },
      {
        id: randomUUID(),
        action: {
          kind: "entry_insert",
          playlist_id: playlist,
          entry_id: randomUUID(),
          recording_id: original.id,
          position: 0,
        },
      },
      {
        id: randomUUID(),
        action: {
          kind: "entry_insert",
          playlist_id: playlist,
          entry_id: randomUUID(),
          recording_id: recording.id,
          position: 1,
        },
      },
    ],
  };
  const saved = await page.request.post("/api/v1/sync", {
    headers,
    data: actions,
  });
  expect(saved.status()).toBe(200);
  const receipt: Schema["SyncReceipt"] = await saved.json();
  expect(
    actions.operations.every((operation) =>
      receipt.acknowledged.includes(operation.id),
    ),
  ).toBe(true);
  const metadata = await read<Schema["RecordingMetadata"]>(
    page,
    `/api/v1/recordings/${recording.id}/metadata`,
  );
  const selection = await read<Schema["PlaylistDetail"]>(
    page,
    `/api/v1/playlists/${playlist}`,
  );
  expect(metadata.title.personal === personalTitle).toBe(true);
  return {
    recording: recording.id,
    personalTitle,
    async assertRetained(reader: Page) {
      const currentMetadata = await read<Schema["RecordingMetadata"]>(
        reader,
        `/api/v1/recordings/${recording.id}/metadata`,
      );
      const currentPlaylist = await read<Schema["PlaylistDetail"]>(
        reader,
        `/api/v1/playlists/${playlist}`,
      );
      const currentOriginals = await read<Schema["RecordingPage"]>(
        reader,
        `/api/v1/recordings?library=${original.library_id}`,
      );
      const currentSources = await read<Schema["Source"][]>(
        reader,
        `/api/v1/recordings/${original.id}/sources`,
      );
      expect(JSON.stringify(currentMetadata) === JSON.stringify(metadata)).toBe(
        true,
      );
      expect(
        JSON.stringify(currentPlaylist) === JSON.stringify(selection),
      ).toBe(true);
      expect(
        JSON.stringify(currentOriginals) === JSON.stringify(originals),
      ).toBe(true);
      expect(
        JSON.stringify(currentSources) === JSON.stringify(originalSources),
      ).toBe(true);
      expect(
        (await originalAudioDigest(reader, originalSources[0].id)) === digest,
      ).toBe(true);
    },
  };
}
