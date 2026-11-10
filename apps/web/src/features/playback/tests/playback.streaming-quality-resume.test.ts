import { expect, test, vi } from "vitest";
import { Playback } from "../playback";
import {
  StreamMedia,
  streamTrack,
  convertedHead,
} from "../fixtures/browserStreamMedia";

function recordingMedia() {
  const media = new StreamMedia();
  const loaded: string[] = [];
  const load = media.load.bind(media);
  vi.spyOn(media, "load").mockImplementation(() => {
    if (media.src) loaded.push(media.src);
    load();
  });
  return { media, loaded };
}

test.each([
  ["high", 26, 26000, 26.006],
  ["saver", 26.0004, 26000, 26.006],
  ["high", 26.0009, 26001, 26.007],
] as const)(
  "%s restored paused resume at %f opens once when its clock advances before metadata",
  async (quality, position, startMs, observed) => {
    localStorage.setItem("hito:stream-quality", quality);
    const requests = vi.fn(async (input: RequestInfo | URL) =>
      convertedHead(input),
    );
    vi.stubGlobal("fetch", requests);
    const { media, loaded } = recordingMedia();
    const player = new Playback(media, async () => [
      { id: "source", ready: true },
    ]);
    expect(player.restoreQueue([streamTrack], 0, position)).toBe(true);
    expect(player.snapshot().playing).toBe(false);
    expect(loaded).toEqual([]);
    await player.setPlaying(true);
    const initial = `/api/v1/sources/source/audio?quality=${quality}&start_ms=${startMs}`;
    expect(loaded).toEqual([initial]);
    media.currentTime = 0.006;
    media.ready(64);
    await vi.waitFor(() => expect(media.paused).toBe(false));
    media.dispatchEvent(new Event("timeupdate"));
    expect(requests).toHaveBeenCalledTimes(1);
    expect(loaded).toEqual([initial]);
    expect(player.positionSnapshot().position).toBeCloseTo(observed, 6);
    expect(player.snapshot().playing).toBe(true);
    player.retire();
  },
);

test.each([
  ["HEAD", 0],
  ["HEAD", 8],
  ["metadata", 40],
  ["metadata", 0],
] as const)(
  "a new seek while resume waits for %s applies target %f instead of the delegated initial position",
  async (phase, target) => {
    localStorage.setItem("hito:stream-quality", "high");
    let release!: () => void;
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    let calls = 0;
    const requests = vi.fn(async (input: RequestInfo | URL) => {
      if (++calls === 1 && phase === "HEAD") await held;
      return convertedHead(input);
    });
    vi.stubGlobal("fetch", requests);
    const { media, loaded } = recordingMedia();
    const player = new Playback(media, async () => [
      { id: "source", ready: true },
    ]);
    player.restoreQueue([streamTrack], 0, 26.0004);
    const opening = player.setPlaying(true);
    await vi.waitFor(() => expect(requests).toHaveBeenCalledTimes(1));
    if (phase === "metadata") await opening;
    player.seek(target);
    release();
    await opening;
    media.currentTime = 0.006;
    media.ready(64);
    const requested = `/api/v1/sources/source/audio?quality=high&start_ms=${target * 1000}`;
    await vi.waitFor(() => expect(media.src).toBe(requested));
    media.currentTime = 0.007;
    media.ready(90 - target);
    await vi.waitFor(() => expect(media.paused).toBe(false));
    media.dispatchEvent(new Event("timeupdate"));
    expect(requests).toHaveBeenCalledTimes(2);
    expect(loaded).toEqual([
      "/api/v1/sources/source/audio?quality=high&start_ms=26000",
      requested,
    ]);
    expect(player.positionSnapshot().position).toBeCloseTo(target + 0.007, 6);
    player.retire();
  },
);

test("Original fallback and authorized source recovery retain resume position without re-opening converted metadata", async () => {
  localStorage.setItem("hito:stream-quality", "high");
  const requests = vi.fn(async (input: RequestInfo | URL) =>
    convertedHead(input),
  );
  vi.stubGlobal("fetch", requests);
  const { media, loaded } = recordingMedia();
  const player = new Playback(media, async (_recording, options) => [
    {
      id: options.purpose === "refresh" ? "replacement" : "source",
      ready: true,
    },
  ]);
  player.restoreQueue([streamTrack], 0, 26.0004);
  await player.setPlaying(true);
  media.currentTime = 0.006;
  media.ready(64);
  media.advance(1);
  media.error = { code: 2 };
  media.dispatchEvent(new Event("error"));
  expect(media.src).toBe("/api/v1/sources/source/audio");
  media.ready();
  expect(media.currentTime).toBe(27);
  media.error = { code: 2 };
  media.dispatchEvent(new Event("error"));
  await vi.waitFor(() =>
    expect(media.src).toBe(
      "/api/v1/sources/replacement/audio?quality=high&start_ms=27000",
    ),
  );
  media.currentTime = 0.006;
  media.ready(63);
  await vi.waitFor(() => expect(media.paused).toBe(false));
  media.dispatchEvent(new Event("timeupdate"));
  expect(requests).toHaveBeenCalledTimes(2);
  expect(loaded).toEqual([
    "/api/v1/sources/source/audio?quality=high&start_ms=26000",
    "/api/v1/sources/source/audio",
    "/api/v1/sources/replacement/audio?quality=high&start_ms=27000",
  ]);
  expect(player.positionSnapshot().position).toBeCloseTo(27.006, 6);
  player.retire();
});
