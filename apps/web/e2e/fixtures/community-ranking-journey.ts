import { randomUUID } from "node:crypto";
import { expect, type Browser, type Page } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";
import {
  publicSources,
  type rankingProviders,
} from "./community-ranking-providers";

type Providers = Awaited<ReturnType<typeof rankingProviders>>;

export async function exchange<T>(
  page: Page,
  path: string,
  account?: string,
  method = "GET",
  data?: unknown,
  status = 200,
): Promise<T> {
  const response = await page.request.fetch(`/api/v1${path}`, {
    method,
    timeout: 5000,
    maxRedirects: 0,
    headers: {
      origin: "http://localhost:8080",
      "x-hito-request": "1",
      ...(account ? { "x-hito-account": account } : {}),
    },
    ...(data === undefined ? {} : { data }),
  });
  expect(
    response.status() === status &&
      response.headers()["cache-control"]?.includes("no-store") &&
      /^[0-9a-f-]{36}$/.test(response.headers()["x-request-id"] ?? ""),
  ).toBe(true);
  const bytes = await response.body();
  expect(bytes.length <= 524_288).toBe(true);
  return (bytes.length ? JSON.parse(bytes.toString("utf8")) : null) as T;
}

export async function prepareRankingLibrary(
  page: Page,
  browser: Browser,
  baseURL: string,
  providers: Providers,
) {
  expect(
    (await exchange<Schema<"SetupStatus">>(page, "/setup")).needs_setup,
  ).toBe(true);
  const password = `private-ranking-password-${randomUUID()}`;
  const owner = await exchange<Schema<"AuthSession">>(
    page,
    "/setup",
    undefined,
    "POST",
    { username: "ranking-owner", password },
    201,
  );
  const account = owner.user.id;
  const installations: Schema<"PluginInstallation">[] = [];
  for (const [index, provider] of providers.providers.entries()) {
    const service = await exchange<Schema<"PluginService">>(
      page,
      "/plugins/services",
      account,
      "POST",
      {
        name: `Owned ranking provider ${index}`,
        base_url: providers.address(index),
      } satisfies Schema<"OfferPluginService">,
      201,
    );
    const request = {
      service_id: service.id,
      service_revision: service.revision,
      bearer_token: provider.token,
    } satisfies Schema<"PreviewPluginInstallation">;
    const preview = await exchange<Schema<"PluginInstallationPreview">>(
      page,
      "/plugins/preview",
      account,
      "POST",
      request,
    );
    expect(
      preview.manifest.id === provider.service &&
        preview.manifest.content_policy?.source_persistence ===
          "playback_only" &&
        preview.digest.length > 0,
    ).toBe(true);
    installations.push(
      await exchange<Schema<"PluginInstallation">>(
        page,
        "/plugins",
        account,
        "POST",
        {
          ...request,
          accept_manifest_digest: preview.digest,
        } satisfies Schema<"InstallPlugin">,
        201,
      ),
    );
  }
  const recording = await exchange<Schema<"Recording">>(
    page,
    `/plugins/${installations[0].id}/recordings`,
    account,
    "POST",
    {
      installation_revision: installations[0].revision,
      provider_recording_id: providers.providers[0].recording,
    } satisfies Schema<"AdoptProviderRecording">,
  );
  expect(!recording.available).toBe(true);
  const resolution = await exchange<Schema<"SourceResolution">>(
    page,
    `/recordings/${recording.id}/sources/resolve`,
    account,
    "POST",
    { request_id: randomUUID() } satisfies Schema<"ResolveSource">,
  );
  expect(
    resolution.recording_id === recording.id &&
      resolution.outcome === "ready" &&
      resolution.sources.length === 2 &&
      resolution.sources.filter((source) => source.ready).length === 1 &&
      providers.providers[1].counts.search === 1 &&
      providers.providers[1].counts.detail === 1 &&
      providers.providers[1].counts.sources === 1,
  ).toBe(true);
  providers.providers[0].ready = true;
  await exchange<Schema<"Source">[]>(
    page,
    `/recordings/${recording.id}/sources/refresh`,
    account,
    "POST",
  );
  const baseline = await exchange<Schema<"Source">[]>(
    page,
    `/recordings/${recording.id}/sources`,
    account,
  );
  expect(
    baseline.length === 2 && baseline.every((source) => source.ready),
  ).toBe(true);
  const refs = baseline.map((source) => {
    const index = providers.providers.findIndex(
      (provider) => provider.label === source.label,
    );
    expect(index === 0 || index === 1).toBe(true);
    return publicSources[index];
  });
  const playlist = randomUUID();
  const playlistName = `private-ranking-list-${randomUUID()}`;
  const personalTitle = `private-ranking-personal-title-${randomUUID()}`;
  const batch = {
    device_id: randomUUID(),
    operations: [
      {
        id: randomUUID(),
        action: {
          kind: "playlist_create",
          playlist_id: playlist,
          name: playlistName,
        },
      },
      {
        id: randomUUID(),
        action: {
          kind: "entry_insert",
          playlist_id: playlist,
          entry_id: randomUUID(),
          recording_id: recording.id,
          position: 0,
        },
      },
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
        action: { kind: "like_set", recording_id: recording.id, liked: true },
      },
    ],
  } satisfies Schema<"SyncBatch">;
  const sync = await exchange<Schema<"SyncReceipt">>(
    page,
    "/sync",
    account,
    "POST",
    batch,
  );
  expect(
    batch.operations.every((operation) =>
      sync.acknowledged.includes(operation.id),
    ),
  ).toBe(true);
  const peer = await exchange<Schema<"User">>(
    page,
    "/users",
    account,
    "POST",
    { username: "ranking-peer", password },
    201,
  );
  const peerContext = await browser.newContext({ baseURL });
  const peerPage = await peerContext.newPage();
  const peerSession = await exchange<Schema<"AuthSession">>(
    peerPage,
    "/login",
    undefined,
    "POST",
    { username: "ranking-peer", password },
  );
  const peerState = async () =>
    JSON.stringify([
      await exchange<Schema<"CommunityRankingPreferences">>(
        peerPage,
        "/community/ranking-preferences",
        peer.id,
      ),
      await exchange<Schema<"RecordingPage">>(peerPage, "/recordings", peer.id),
      await exchange<Schema<"Playlist">[]>(peerPage, "/playlists", peer.id),
      await exchange<Schema<"Activity">>(peerPage, "/activity", peer.id),
      await exchange<Schema<"PluginSettings">>(peerPage, "/plugins", peer.id),
    ]);
  const identity = async () =>
    JSON.stringify([
      await exchange<unknown>(
        page,
        `/recordings/${recording.id}/metadata`,
        account,
      ),
      await exchange<Schema<"PlaylistDetail">>(
        page,
        `/playlists/${playlist}`,
        account,
      ),
      await exchange<Schema<"PluginSettings">>(page, "/plugins", account),
    ]);
  return {
    account,
    recording,
    baseline,
    refs,
    playlist,
    personalTitle,
    identity,
    peerState,
    peerContext,
    privateValues: [
      password,
      personalTitle,
      playlistName,
      owner.token,
      peerSession.token,
    ],
  };
}
