import { createHash } from "node:crypto";
import { readFile, stat } from "node:fs/promises";
import { expect, type Page } from "@playwright/test";
import { isDeepStrictEqual } from "node:util";
import type { Schema } from "../../src/shared/schema";
import {
  artistReviewDownloadAbsence,
  artistReviewReader,
} from "./artist-review-state";
import { preferencesAction } from "./metadata-preferences-state";
import { reviewPluginInstallation } from "./plugin-installation-review";
import type { recordingLyricsProvider } from "./recording-lyrics-provider";

type Provider = Awaited<ReturnType<typeof recordingLyricsProvider>>;

export async function lyricDeadline<T>(
  work: Promise<T>,
  milliseconds = 5000,
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      work,
      new Promise<never>((_resolve, reject) => {
        timer = setTimeout(
          () => reject(new Error("Owned lyric fixture deadline exceeded")),
          milliseconds,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

export async function connectLyricsProvider(page: Page, owned: Provider) {
  await page
    .getByRole("link", { name: "Account and settings", exact: true })
    .click();
  await page
    .getByRole("navigation", { name: "Settings sections" })
    .getByRole("link", { name: "Plugins", exact: true })
    .click();
  await page.getByRole("button", { name: /^This server/ }).click();
  await page.getByLabel("Service address", { exact: true }).fill(owned.address);
  const offered = await preferencesAction<Schema<"PluginService">>(
    page,
    page.getByRole("button", { name: "Offer service", exact: true }),
    "POST",
    "/api/v1/plugins/services",
    201,
  );
  if (!offered.body)
    throw new Error("The owned lyric service offer is missing");
  expect(
    isDeepStrictEqual(offered.sent, { base_url: owned.address }) &&
      offered.body.base_url === owned.address &&
      offered.body.name === owned.manifest.name,
  ).toBe(true);
  await page.getByRole("button", { name: "Your plugins", exact: true }).click();
  await page
    .getByRole("button", { name: `Connect ${offered.body.name}`, exact: true })
    .click();
  await page
    .getByLabel("Bearer token (optional)", { exact: true })
    .fill(owned.token);
  const preview = await reviewPluginInstallation(
    page,
    offered.body,
    owned.manifest.name,
  );
  const installed = await preferencesAction<Schema<"PluginInstallation">>(
    page,
    page.getByRole("button", { name: "Accept and connect", exact: true }),
    "POST",
    "/api/v1/plugins",
    201,
  );
  if (!installed.body)
    throw new Error("The owned lyric installation is missing");
  expect(
    installed.body.service_id === offered.body.id &&
      installed.body.credential_set &&
      isDeepStrictEqual(installed.body.manifest, owned.manifest) &&
      isDeepStrictEqual(installed.sent, {
        ...preview.sent,
        accept_manifest_digest: preview.body.digest,
      }),
  ).toBe(true);
  return installed.body;
}

export async function lyricState(page: Page, ids: string[], playlist: string) {
  const read = artistReviewReader(page);
  const response = await page.request.post("/api/v1/recordings/progress", {
    headers: { "x-hito-request": "1", Origin: new URL(page.url()).origin },
    data: { recording_ids: ids } satisfies Schema<"SavedPlaybackRequest">,
    timeout: 10_000,
  });
  let progress: Schema<"RecordingProgress">[];
  try {
    const bytes = await response.body();
    expect(response.status() === 200 && bytes.length <= 16_384).toBe(true);
    progress = JSON.parse(bytes.toString());
  } finally {
    await response.dispose();
  }
  return {
    progress,
    catalog: await read<Schema<"RecordingPage">>("/api/v1/recordings"),
    plugins: await read<Schema<"PluginSettings">>("/api/v1/plugins"),
    playlist: await read<Schema<"PlaylistDetail">>(
      `/api/v1/playlists/${playlist}`,
    ),
    activity: await read<Schema<"Activity">>("/api/v1/activity"),
    events: await read<Schema<"SyncPage">>("/api/v1/sync?cursor=0"),
    metadata: await Promise.all(
      ids.map((id) =>
        read<Schema<"RecordingMetadata">>(`/api/v1/recordings/${id}/metadata`),
      ),
    ),
    sources: await Promise.all(
      ids.map((id) =>
        read<Schema<"Source">[]>(`/api/v1/recordings/${id}/sources`),
      ),
    ),
    downloads: await artistReviewDownloadAbsence(page, ids),
  };
}

export async function lyricOriginals() {
  const originals = [];
  for (const path of [
    "Music/Fixture 440 Hz.wav",
    "Archive/Fixture 660 Hz.wav",
  ]) {
    const filename = `/workspace/fixtures/e2e-generated/${path}`;
    const metadata = await stat(filename);
    if (
      !metadata.isFile() ||
      metadata.size < 44 ||
      metadata.size > 8 * 1024 * 1024
    )
      throw new Error("Owned lyric original exceeds its bound");
    const bytes = await readFile(filename);
    expect(
      bytes.length === metadata.size &&
        bytes.subarray(0, 4).toString() === "RIFF" &&
        bytes.subarray(8, 12).toString() === "WAVE",
    ).toBe(true);
    originals.push({
      size: metadata.size,
      modified: metadata.mtimeMs,
      mode: metadata.mode,
      uid: metadata.uid,
      gid: metadata.gid,
      sha256: createHash("sha256").update(bytes).digest("hex"),
    });
  }
  return originals;
}

export async function assertLyricReply(
  page: Page,
  recording: string,
  observation: string,
  revision: number,
  click: () => Promise<void>,
) {
  const [response] = await Promise.all([
    page.waitForResponse(
      (reply) =>
        new URL(reply.url()).pathname ===
          `/api/v1/recordings/${recording}/lyrics` &&
        reply.request().method() === "POST",
      { timeout: 15_000 },
    ),
    click(),
  ]);
  const bytes = await response.body();
  expect(
    response.status() === 200 &&
      response.headers()["cache-control"] === "private, no-store" &&
      bytes.length <= 262_144 &&
      isDeepStrictEqual(response.request().postDataJSON(), {
        observation_id: observation,
        installation_revision: revision,
      }),
  ).toBe(true);
  return JSON.parse(bytes.toString()) as Schema<"LyricsLookup">;
}
