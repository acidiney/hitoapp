import { isDeepStrictEqual } from "node:util";
import {
  expect,
  type Page,
  type Request,
  type Response,
} from "@playwright/test";
import type { Schema } from "../../src/shared/schema";

const mediaPrecisionMs = 100;
const uuid =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

type HeardMedia = { position_ms: number; prefix_ms: number; paused: boolean };
type HttpReceipt = { request_id: string; status: number; route: string };
export type OwnedProgressWrite = {
  batch: Schema<"SyncBatch">;
  observed: Promise<HeardMedia>;
  receipt?: HttpReceipt;
};
export type OwnedPlaybackEpisode = {
  recording: string;
  source: string;
  writes: OwnedProgressWrite[];
  beforePause?: HeardMedia;
  afterPause?: HeardMedia;
};

async function heardMedia(page: Page, source: string): Promise<HeardMedia> {
  const sample = await page.locator("audio[data-hito-player]").evaluate(
    (audio: HTMLAudioElement, expected) => ({
      current: audio.currentTime,
      paused: audio.paused,
      valid:
        new URL(audio.currentSrc || audio.src).pathname === expected &&
        audio.readyState >= HTMLMediaElement.HAVE_CURRENT_DATA &&
        audio.playbackRate === 1 &&
        !audio.seeking &&
        !audio.ended &&
        audio.error === null,
      played: Array.from(
        { length: Math.min(audio.played.length, 2) },
        (_, i) => [audio.played.start(i), audio.played.end(i)],
      ),
    }),
    `/api/v1/sources/${source}/audio`,
  );
  expect(
    sample.valid &&
      Number.isFinite(sample.current) &&
      sample.current >= 0 &&
      sample.played.length === 1 &&
      sample.played[0].every(Number.isFinite) &&
      sample.played[0][0] >= 0 &&
      sample.played[0][0] * 1000 <= mediaPrecisionMs &&
      sample.played[0][1] > sample.played[0][0] &&
      Math.abs(sample.current - sample.played[0][1]) * 1000 <= mediaPrecisionMs,
  ).toBe(true);
  return {
    position_ms: Math.floor(sample.current * 1000),
    prefix_ms: Math.floor(sample.played[0][1] * 1000),
    paused: sample.paused,
  };
}

export function assertHeardProgress(
  batch: Schema<"SyncBatch">,
  recording: string,
  heard: HeardMedia,
) {
  expect(
    uuid.test(batch.device_id) &&
      batch.operations.length > 0 &&
      batch.operations.length <= 2 &&
      new Set(batch.operations.map(({ id }) => id)).size ===
        batch.operations.length &&
      batch.operations.every(
        ({ id, action }) =>
          uuid.test(id) &&
          action.kind === "progress_set" &&
          action.recording_id === recording &&
          action.speed === 1 &&
          Number.isSafeInteger(action.position_ms) &&
          action.position_ms > 0 &&
          action.position_ms <=
            Math.min(heard.position_ms, heard.prefix_ms) + mediaPrecisionMs,
      ),
  ).toBe(true);
}

export function observeOwnedProgress(
  page: Page,
  request: Request,
  episode: OwnedPlaybackEpisode | undefined,
): OwnedProgressWrite {
  const raw = request.postData();
  if (
    !episode ||
    !raw ||
    Buffer.byteLength(raw) > 4096 ||
    episode.writes.length >= 4
  )
    throw new Error("Unexpected owned progress request");
  const batch: Schema<"SyncBatch"> = JSON.parse(raw);
  const observed = heardMedia(page, episode.source).then((sample) => {
    assertHeardProgress(batch, episode.recording, sample);
    return sample;
  });
  const write = { batch, observed };
  episode.writes.push(write);
  return write;
}

export async function acknowledgeOwnedProgress(
  response: Response,
  write: OwnedProgressWrite | undefined,
): Promise<HttpReceipt> {
  if (!write || write.receipt)
    throw new Error("Unexpected owned progress response");
  await write.observed;
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    const bytes = await Promise.race([
      response.body(),
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () =>
            reject(new Error("Owned progress receipt exceeded its deadline")),
          5000,
        );
      }),
    ]);
    expect(response.status() === 200 && bytes.length <= 4096).toBe(true);
    const receipt: Schema<"SyncReceipt"> = JSON.parse(bytes.toString("utf8"));
    expect(
      isDeepStrictEqual(
        receipt.acknowledged,
        write.batch.operations.map(({ id }) => id),
      ),
    ).toBe(true);
  } finally {
    clearTimeout(timer);
  }
  const headers = await response.allHeaders();
  expect(
    headers["cache-control"]?.includes("no-store") &&
      uuid.test(headers["x-request-id"] ?? ""),
  ).toBe(true);
  write.receipt = {
    request_id: headers["x-request-id"],
    status: response.status(),
    route: "/api/v1/sync",
  };
  return write.receipt;
}

export async function pauseOwnedPlayback(
  page: Page,
  episode: OwnedPlaybackEpisode,
) {
  episode.beforePause = await heardMedia(page, episode.source);
  expect(
    !episode.beforePause.paused && episode.beforePause.position_ms >= 300,
  ).toBe(true);
  await page
    .getByRole("button", { name: "Pause", exact: true })
    .click({ timeout: 1000 });
  episode.afterPause = await heardMedia(page, episode.source);
  expect(
    episode.afterPause.paused &&
      episode.afterPause.position_ms >=
        episode.beforePause.position_ms - mediaPrecisionMs &&
      episode.afterPause.position_ms <=
        episode.beforePause.position_ms + 1000 + mediaPrecisionMs,
  ).toBe(true);
  await expect
    .poll(() => episode.writes.length === 1 && !!episode.writes[0].receipt, {
      timeout: 5000,
    })
    .toBe(true);
  const write = episode.writes[0];
  assertHeardProgress(write.batch, episode.recording, episode.afterPause);
  expect(
    write.batch.operations.every(
      ({ action }) =>
        action.kind === "progress_set" &&
        action.position_ms >=
          Math.max(200, episode.beforePause!.position_ms - mediaPrecisionMs),
    ),
  ).toBe(true);
}
