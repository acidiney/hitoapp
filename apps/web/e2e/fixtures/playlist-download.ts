import { createHash, randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";
import { expect, type Page } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";
import {
  prepareDownloadLibrary,
  read,
  type DownloadProvider,
} from "./downloads";

export async function preparePlaylistDownload(
  page: Page,
  baseURL: string,
  provider: DownloadProvider,
) {
  const library = await prepareDownloadLibrary(page, baseURL, provider);
  const playlist = randomUUID();
  const entryIds = [randomUUID(), randomUUID()];
  const batch: Schema<"SyncBatch"> = {
    device_id: "playlist-download-browser-fixture",
    operations: [
      {
        id: randomUUID(),
        action: {
          kind: "playlist_create",
          playlist_id: playlist,
          name: "Owned duplicate download selection",
        },
      },
      ...entryIds.map((entry, position) => ({
        id: randomUUID(),
        action: {
          kind: "entry_insert" as const,
          playlist_id: playlist,
          entry_id: entry,
          recording_id: library.recording,
          position,
        },
      })),
    ],
  };
  const response = await page.request.post("/api/v1/sync", {
    headers: { "x-hito-request": "1", origin: new URL(baseURL).origin },
    data: batch,
  });
  expect(response.status()).toBe(200);
  const receipt: Schema<"SyncReceipt"> = await response.json();
  expect(
    batch.operations.every((operation) =>
      receipt.acknowledged.includes(operation.id),
    ),
  ).toBe(true);
  const detailPath = `/api/v1/playlists/${playlist}`;
  const downloadPath = `${detailPath}/download`;
  const recordingDownloadPath = `/api/v1/recordings/${library.recording}/download`;
  const detail = await read<Schema<"PlaylistDetail">>(page, detailPath);
  const catalog = await read<Schema<"RecordingPage">>(
    page,
    "/api/v1/recordings",
  );
  expect(
    detail.entries.length === 2 &&
      detail.entries.every(
        (entry, index) =>
          entry.entry_id === entryIds[index] &&
          entry.recording_id === library.recording,
      ),
  ).toBe(true);
  const owned = await readFile(
    "/workspace/fixtures/e2e-generated/Music/Fixture 440 Hz.wav",
  );
  const ownedDigest = createHash("sha256").update(owned).digest("hex");
  return {
    playlist,
    recording: library.recording,
    entryIds,
    detailPath,
    downloadPath,
    recordingDownloadPath,
    async assertNoIndividualDownload(reader: Page) {
      expect((await reader.request.get(recordingDownloadPath)).status()).toBe(
        404,
      );
    },
    async state(reader: Page, retention: string, status: "failed" | "ready") {
      const current = await read<Schema<"PlaylistDownloadStatus">>(
        reader,
        downloadPath,
      );
      expect(
        current.retention_id === retention &&
          current.playlist_id === playlist &&
          current.desired_revision === current.reconciled_revision &&
          current.entries.next_cursor == null &&
          current.entries.items.length === 2 &&
          entryIds.every((entry) =>
            current.entries.items.some(
              (item) =>
                item.entry_id === entry &&
                item.recording_id === library.recording &&
                item.status === status,
            ),
          ),
      ).toBe(true);
      return current;
    },
    async assertOwnedAudio(reader: Page, source: string) {
      const audio = await reader.request.get(`/api/v1/sources/${source}/audio`);
      expect(audio.status()).toBe(200);
      expect(audio.headers()["content-type"] === "audio/wav").toBe(true);
      expect(
        createHash("sha256")
          .update(await audio.body())
          .digest("hex") === ownedDigest,
      ).toBe(true);
    },
    async assertRetained(reader: Page) {
      await library.assertRetained(reader);
      const currentDetail = await read<Schema<"PlaylistDetail">>(
        reader,
        detailPath,
      );
      const currentCatalog = await read<Schema<"RecordingPage">>(
        reader,
        "/api/v1/recordings",
      );
      expect(JSON.stringify(currentDetail) === JSON.stringify(detail)).toBe(
        true,
      );
      expect(JSON.stringify(currentCatalog) === JSON.stringify(catalog)).toBe(
        true,
      );
    },
  };
}
