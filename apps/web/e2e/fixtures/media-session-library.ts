import { randomUUID } from "node:crypto";
import { expect, type Page } from "@playwright/test";
import type { components } from "../../src/generated";
import { signIn } from "./artwork-browser-checks";
import type { mediaSessionProvider } from "./media-session-provider";

type Schema = components["schemas"];

export async function prepareMediaSessionLibrary(
  page: Page,
  origin: string,
  fixture: Awaited<ReturnType<typeof mediaSessionProvider>>,
) {
  const headers = { "x-hito-request": "1", origin };
  await signIn(page);
  const offered = await page.request.post("/api/v1/plugins/services", {
    headers,
    data: {
      name: "Owned media session service",
      base_url: fixture.address,
    } satisfies Schema["OfferPluginService"],
  });
  expect(offered.status()).toBe(201);
  const service: Schema["PluginService"] = await offered.json();
  const installed = await page.request.post("/api/v1/plugins", {
    headers,
    data: {
      service_id: service.id,
      service_revision: service.revision,
      bearer_token: fixture.token,
    } satisfies Schema["InstallPlugin"],
  });
  expect(installed.status()).toBe(201);
  const installation: Schema["PluginInstallation"] = await installed.json();
  const records: Schema["Recording"][] = [];
  const expectedSources: string[] = [];
  for (const track of fixture.tracks) {
    const response = await page.request.post(
      `/api/v1/plugins/${installation.id}/recordings`,
      {
        headers,
        data: {
          installation_revision: installation.revision,
          provider_recording_id: track.id,
        } satisfies Schema["AdoptProviderRecording"],
      },
    );
    expect(response.status()).toBe(200);
    const record: Schema["Recording"] = await response.json();
    expect(
      record.title === track.title &&
        record.artist === track.artist &&
        record.id !== track.id,
    ).toBe(true);
    records.push(record);
    const cached = await page.request.get(
      `/api/v1/recordings/${record.id}/sources`,
    );
    expect(cached.status()).toBe(200);
    const sources: Schema["Source"][] = await cached.json();
    expect(
      sources.length === 1 &&
        sources[0].ready &&
        sources[0].recording_id === record.id &&
        /^[0-9a-f-]{36}$/.test(sources[0].id),
    ).toBe(true);
    expectedSources.push(sources[0].id);
  }
  const playlist = randomUUID();
  const operations: Schema["SyncBatch"]["operations"] = [
    {
      id: randomUUID(),
      action: {
        kind: "playlist_create",
        playlist_id: playlist,
        name: "Owned system playback queue",
      },
    },
    ...records.map((record, position) => ({
      id: randomUUID(),
      action: {
        kind: "entry_insert" as const,
        playlist_id: playlist,
        entry_id: randomUUID(),
        recording_id: record.id,
        position,
      },
    })),
  ];
  const saved = await page.request.post("/api/v1/sync", {
    headers,
    data: {
      device_id: "owned-browser-media-session",
      operations,
    } satisfies Schema["SyncBatch"],
  });
  expect(saved.status()).toBe(200);
  const receipt: Schema["SyncReceipt"] = await saved.json();
  expect(
    operations.every((operation) =>
      receipt.acknowledged.includes(operation.id),
    ),
  ).toBe(true);
  await page.goto(`/playlists/${playlist}`);
  return { records, playlist, expectedSources };
}
