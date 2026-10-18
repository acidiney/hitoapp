import { isDeepStrictEqual } from "node:util";
import type { APIRequestContext, Page } from "@playwright/test";
import type { components } from "../../src/generated";
import {
  liveCheck,
  normalizedLiveRecording,
  readSpotifyAlbumsLive,
} from "./spotify-albums-live";
import type { PreparedSpotifyUpgrade } from "./spotify-metadata-upgrade-setup";

export type UpgradeSchema = components["schemas"];
export type UpgradeRead = <T>(path: string) => Promise<T>;
export type UpgradeLive = Awaited<ReturnType<typeof readSpotifyAlbumsLive>>;

export async function upgradePost<T>(
  page: Page,
  path: string,
  click: () => Promise<void>,
) {
  const [response] = await Promise.all([
    page.waitForResponse(
      (candidate) =>
        new URL(candidate.url()).pathname === path &&
        candidate.request().method() === "POST",
      { timeout: 75_000 },
    ),
    click(),
  ]);
  liveCheck(response.status() === 200);
  const bytes = await response.body();
  liveCheck(bytes.length <= 32_768);
  return {
    body: JSON.parse(bytes.toString()) as T,
    bytes,
    sent: response.request().postDataJSON() as unknown,
  };
}

export async function upgradeManifest(request: APIRequestContext) {
  const response = await request.get("v1/manifest", {
    timeout: 10_000,
    maxRetries: 0,
    maxRedirects: 0,
  });
  try {
    liveCheck(response.status() === 200);
    const bytes = await response.body();
    liveCheck(bytes.length <= 32_768);
    const manifest = JSON.parse(bytes.toString()) as UpgradeSchema["Manifest"];
    liveCheck(
      manifest.id === "org.hito.spotify" &&
        manifest.name === "Spotify public playlists" &&
        manifest.protocol_version === 1 &&
        manifest.authorization == null &&
        manifest.content_policy == null &&
        isDeepStrictEqual(manifest.capabilities, ["playlists", "artwork"]),
    );
    return { bytes, manifest };
  } finally {
    await response.dispose();
  }
}

export async function upgradeReplay(
  page: Page,
  account: string,
  path: string,
  data: unknown,
  expected: Buffer,
) {
  const response = await page.request.post(path, {
    headers: { "x-hito-request": "1", "x-hito-account": account },
    data,
    timeout: 15_000,
    maxRedirects: 0,
    maxRetries: 0,
  });
  try {
    liveCheck(response.status() === 200);
    const bytes = await response.body();
    liveCheck(bytes.length <= 32_768 && bytes.equals(expected));
  } finally {
    await response.dispose();
  }
}

export async function upgradeEvents(read: UpgradeRead) {
  const events: UpgradeSchema["SyncEvent"][] = [];
  let cursor = 0;
  for (let page = 0; page < 32; page++) {
    const result = await read<UpgradeSchema["SyncPage"]>(
      `/api/v1/sync?cursor=${cursor}`,
    );
    liveCheck(
      result.operations.every(
        (event, index) =>
          event.sequence >
          (index === 0 ? cursor : result.operations[index - 1].sequence),
      ),
    );
    events.push(...result.operations);
    liveCheck(events.length <= 64 && result.cursor >= cursor);
    if (!result.has_more) return events;
    liveCheck(result.cursor > cursor);
    cursor = result.cursor;
  }
  throw new Error("The isolated event inspection exceeded its bound");
}

export async function upgradeCatalog(read: UpgradeRead, total: number) {
  liveCheck(total > 0 && total <= 10_000);
  const items: UpgradeSchema["Recording"][] = [];
  for (let offset = 0; offset < total; offset += 200) {
    const page = await read<UpgradeSchema["RecordingPage"]>(
      `/api/v1/recordings?limit=200&offset=${offset}`,
    );
    liveCheck(
      page.total === total &&
        page.offset === offset &&
        page.limit === 200 &&
        page.items.length > 0,
    );
    items.push(...page.items);
  }
  liveCheck(
    items.length === total &&
      new Set(items.map((item) => item.id)).size === total,
  );
  return items;
}

export async function spotifyUpgradeState(
  read: UpgradeRead,
  prepared: PreparedSpotifyUpgrade,
) {
  const id = prepared.selected;
  return {
    user: await read<UpgradeSchema["User"]>("/api/v1/me"),
    settings: await read<UpgradeSchema["PluginSettings"]>("/api/v1/plugins"),
    playlists: await read<UpgradeSchema["Playlist"][]>("/api/v1/playlists"),
    copy: await read<UpgradeSchema["PlaylistDetail"]>(
      `/api/v1/playlists/${prepared.playlist}`,
    ),
    catalog: await upgradeCatalog(read, prepared.mapped.recordings.size),
    albums: await read<UpgradeSchema["ReleasePage"]>(
      "/api/v1/releases?limit=200",
    ),
    activity: await read<UpgradeSchema["Activity"]>("/api/v1/activity"),
    events: await upgradeEvents(read),
    metadata: await read<UpgradeSchema["RecordingMetadata"]>(
      `/api/v1/recordings/${id}/metadata`,
    ),
    sources: await read<UpgradeSchema["Source"][]>(
      `/api/v1/recordings/${id}/sources`,
    ),
    choice: await read<UpgradeSchema["SourceChoice"]>(
      `/api/v1/recordings/${id}/source-choice`,
    ),
  };
}

export function rememberSpotifyValues(values: Set<string>, live: UpgradeLive) {
  for (const value of [live.detail.name, live.detail.snapshot, live.detail.id])
    values.add(value);
  for (const item of live.recordings.values()) {
    const normalized = normalizedLiveRecording(item);
    for (const value of [
      normalized.id,
      normalized.title,
      normalized.artist,
      normalized.release?.id,
      normalized.release?.title,
      normalized.release?.artist,
    ])
      if (value) values.add(value);
  }
}
