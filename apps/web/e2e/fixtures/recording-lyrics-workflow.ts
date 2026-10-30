import { expect, type Page } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";
import { artistReviewReader } from "./artist-review-state";
import { seedPreferencesLibrary } from "./metadata-preferences-state";
import {
  discoverRecording,
  openSavedRecording,
} from "./recording-attribution-state";
import type { recordingLyricsProvider } from "./recording-lyrics-provider";
import { connectLyricsProvider, lyricState } from "./recording-lyrics-state";

export async function prepareLyricsLibrary(
  page: Page,
  owned: Awaited<ReturnType<typeof recordingLyricsProvider>>,
  query: string,
  privateValues: string[],
) {
  const read = artistReviewReader(page);
  const user = await read<Schema<"User">>("/api/v1/me");
  const local = await seedPreferencesLibrary(page, read, user);
  privateValues.push(...local.privateValues);
  const connection = await connectLyricsProvider(page, owned);
  const saved: Schema<"Recording">[] = [];
  for (const index of [0, 1]) {
    const origin = new URL(page.url()).origin;
    const known = new Set(
      [...local.ids, ...saved.map((item) => item.id)].map(
        (id) => `${origin}/api/v1/recordings/${id}/artwork`,
      ),
    );
    const imageReply = page.waitForResponse(
      (reply) =>
        reply.request().resourceType() === "image" &&
        reply.request().method() === "GET" &&
        new URL(reply.url()).origin === origin &&
        /^\/api\/v1\/recordings\/[^/]+\/artwork$/.test(
          new URL(reply.url()).pathname,
        ) &&
        !known.has(reply.url()),
      { timeout: 10_000 },
    );
    const availabilityReply = page.waitForResponse(
      (reply) =>
        reply.request().resourceType() === "fetch" &&
        reply.request().method() === "GET" &&
        new URL(reply.url()).origin === origin &&
        /^\/api\/v1\/recordings\/[^/]+\/artwork\/availability$/.test(
          new URL(reply.url()).pathname,
        ) &&
        !known.has(reply.url().replace(/\/availability$/, "")),
      { timeout: 10_000 },
    );
    const [adopted, image, availability] = await Promise.all([
      discoverRecording(
        page,
        `${query}-${index}`,
        connection,
        owned.recordings[index],
      ),
      imageReply,
      availabilityReply,
    ]);
    const bytes = await image.body();
    expect(
      image.url() === `${origin}/api/v1/recordings/${adopted.id}/artwork` &&
        image.status() === 404 &&
        bytes.length <= 4096 &&
        bytes.toString() === '{"error":"Resource not found"}',
    ).toBe(true);
    const availableBytes = await availability.body();
    expect(
      availability.url() ===
        `${origin}/api/v1/recordings/${adopted.id}/artwork/availability` &&
        availability.status() === 200 &&
        availableBytes.length <= 4096 &&
        availableBytes.toString() === '{"can_fetch":false}',
    ).toBe(true);
    saved.push(adopted);
  }
  const recording = saved[1];
  expect(new Set(saved.map((item) => item.id)).size === 2).toBe(true);
  const document = await page.evaluate(() => performance.timeOrigin);
  await openSavedRecording(page, recording, owned.recordings[1].title);
  expect((await page.evaluate(() => performance.timeOrigin)) === document).toBe(
    true,
  );
  const metadata = await read<Schema<"RecordingMetadata">>(
    `/api/v1/recordings/${recording.id}/metadata`,
  );
  const observation = metadata.selected_observation;
  expect(
    observation?.origin === "provider_response" &&
      observation.plugin_installation_id === connection.id &&
      observation.provider_recording_id === owned.recordings[1].id &&
      observation.provider_revision === owned.recordings[1].revision &&
      observation.source_id === null,
  ).toBe(true);
  if (!observation) throw new Error("The owned lyric observation is missing");
  const ids = [...local.ids, ...saved.map((item) => item.id)];
  const before = await lyricState(page, ids, local.playlist);
  expect(
    before.catalog.total === 4 &&
      before.playlist.entries.length === 2 &&
      before.playlist.entries.every(
        (item) => item.recording_id === local.ids[0],
      ) &&
      before.catalog.items.some(
        (item) => item.id === local.ids[0] && item.liked,
      ) &&
      before.metadata[0].title.personal !== null &&
      before.sources.slice(2).every((items) => items.length === 0) &&
      before.progress.some(
        (item) =>
          item.recording_id === local.ids[0] &&
          item.position_ms === 400 &&
          item.speed === 1.25,
      ),
  ).toBe(true);
  const providerBefore = { ...owned.calls };
  const lyrics = page.getByRole("region", { name: "Lyrics", exact: true });
  await expect(
    lyrics.getByRole("button", { name: "Show lyrics", exact: true }),
  ).toBeVisible();
  expect(owned.calls.lyrics === 0 && owned.calls.descriptor === 0).toBe(true);
  return {
    user,
    local,
    connection,
    saved,
    recording,
    observation,
    document,
    ids,
    before,
    providerBefore,
    lyrics,
  };
}
