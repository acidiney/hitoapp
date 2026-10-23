import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { Playback, type MediaPort, type Track } from "../playback";
import type { BrowserMediaOwner } from "../browser/BrowserPreparation";
import { log } from "../../../shared/log";

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

class Media extends EventTarget implements MediaPort {
  src = "";
  currentSrc = "";
  currentTime = 0;
  duration = 440;
  playbackRate = 1;
  defaultPlaybackRate = 1;
  volume = 1;
  paused = true;
  readyState = 0;
  seeking = false;
  async play() {
    this.paused = false;
    this.dispatchEvent(new Event("play"));
  }
  pause() {
    this.paused = true;
    this.dispatchEvent(new Event("pause"));
  }
  load() {
    this.currentSrc = "";
    this.currentTime = 0;
  }
  removeAttribute() {
    this.src = "";
  }
  metadata() {
    this.currentSrc = this.src;
    this.readyState = 2;
    this.dispatchEvent(new Event("loadedmetadata"));
  }
  tick(seconds: number) {
    this.currentTime = seconds;
    this.dispatchEvent(new Event("timeupdate"));
  }
}

const track = (id: string, durationMs = 0): Track => ({
  id,
  title: "Owned overlong fixture",
  artist: "Owned performer",
  duration_ms: durationMs,
  available: true,
});
const timed = (id: string) => track(id, 220_000);

async function start(
  mediaSeconds: number,
  tracks: Track[] = [timed("first"), timed("next")],
) {
  const media = new Media();
  media.duration = mediaSeconds;
  const resolve = vi.fn(async (id: string) => [
    { id: `source-${id}`, ready: true },
  ]);
  const progress = vi.fn();
  const player = new Playback(media, resolve, progress);
  await player.playQueue(tracks);
  media.metadata();
  return { media, resolve, progress, player };
}
const opened = (resolve: ReturnType<typeof vi.fn>, id: string) =>
  resolve.mock.calls.filter(([requested]) => requested === id).length;

test("an overlong browser estimate yields to the server duration for timeline, seek and logs", async () => {
  const logs = vi.spyOn(log, "debug");
  const { media, player } = await start(440);
  expect(player.positionSnapshot()).toEqual({ position: 0, duration: 220 });
  player.seek(400);
  expect(media.currentTime).toBe(220);
  expect(player.positionSnapshot().position).toBe(220);
  const preferred = logs.mock.calls.filter(
    ([event]) => event === "playback.duration_server_preferred",
  );
  expect(preferred).toEqual([
    [
      "playback.duration_server_preferred",
      {
        recording_id: "first",
        media_duration_ms: 440_000,
        server_duration_ms: 220_000,
      },
    ],
  ]);
});

test.each([
  ["within ten percent", 230],
  ["within five seconds", 224],
  ["exactly at the margin", 242],
])(
  "a browser duration %s of the server value stays authoritative",
  async (_name, seconds) => {
    const { player, media } = await start(seconds);
    expect(player.positionSnapshot().duration).toBe(seconds);
    media.tick(seconds - 0.1);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(player.snapshot().current?.id).toBe("first");
  },
);

test("a shorter measured browser duration wins and ends through the media ended event", async () => {
  const { player, media, resolve } = await start(200);
  expect(player.positionSnapshot().duration).toBe(200);
  media.tick(199.9);
  expect(player.snapshot().current?.id).toBe("first");
  media.dispatchEvent(new Event("ended"));
  await vi.waitFor(() => expect(player.snapshot().current?.id).toBe("next"));
  expect(opened(resolve, "next")).toBe(1);
});

test("reaching the server duration advances once and a late ended event cannot advance again", async () => {
  const { player, media, resolve, progress } = await start(440, [
    timed("first"),
    timed("next"),
    timed("last"),
  ]);
  media.tick(100);
  expect(player.snapshot().current?.id).toBe("first");
  media.tick(219.9);
  await vi.waitFor(() => expect(player.snapshot().current?.id).toBe("next"));
  media.metadata();
  media.dispatchEvent(new Event("ended"));
  await vi.advanceTimersByTimeAsync(10_000);
  expect(player.snapshot().current?.id).toBe("next");
  expect(opened(resolve, "next")).toBe(1);
  expect(opened(resolve, "last")).toBe(0);
  expect(progress.mock.calls.at(-1)?.[0].id).toBe("first");
  expect(progress.mock.calls.at(-1)?.[1]).toBeGreaterThanOrEqual(219.9);
});

test("a late ended event during the next source resolution is ignored", async () => {
  const { player, media, resolve } = await start(440, [
    timed("first"),
    timed("next"),
    timed("last"),
  ]);
  media.tick(220);
  media.dispatchEvent(new Event("ended"));
  media.dispatchEvent(new Event("ended"));
  await vi.waitFor(() => expect(player.snapshot().current?.id).toBe("next"));
  expect(opened(resolve, "next")).toBe(1);
  expect(opened(resolve, "last")).toBe(0);
});

test("a playback stall within the final seconds of the server duration ends the entry once", async () => {
  const { player, media, resolve } = await start(440, [
    timed("first"),
    timed("next"),
    timed("last"),
  ]);
  await media.play();
  media.tick(218.5);
  await vi.advanceTimersByTimeAsync(2_000);
  expect(player.snapshot().current?.id).toBe("first");
  await vi.advanceTimersByTimeAsync(1_000);
  await vi.waitFor(() => expect(player.snapshot().current?.id).toBe("next"));
  await vi.advanceTimersByTimeAsync(20_000);
  expect(opened(resolve, "next")).toBe(1);
  expect(opened(resolve, "last")).toBe(0);
});

test("a pause or seek away from the end cancels the stall watch", async () => {
  const { player, media } = await start(440);
  await media.play();
  media.tick(218.5);
  media.pause();
  await vi.advanceTimersByTimeAsync(10_000);
  expect(player.snapshot().current?.id).toBe("first");
  await media.play();
  media.tick(218.6);
  media.dispatchEvent(new Event("seeking"));
  media.tick(100);
  await vi.advanceTimersByTimeAsync(10_000);
  expect(player.snapshot().current?.id).toBe("first");
});

test("repeat one restarts the same recording when the server duration is reached", async () => {
  const { player, media, resolve } = await start(440);
  player.repeat();
  player.repeat();
  expect(player.snapshot().repeat).toBe("one");
  media.tick(220);
  await vi.waitFor(() => expect(opened(resolve, "first")).toBe(2));
  expect(player.snapshot().current?.id).toBe("first");
  expect(player.snapshot().index).toBe(0);
  media.metadata();
  media.dispatchEvent(new Event("ended"));
  await vi.advanceTimersByTimeAsync(1_000);
  expect(opened(resolve, "first")).toBe(2);
});

test("the end of the queue stops cleanly at the server duration", async () => {
  const { player, media } = await start(440, [timed("only")]);
  media.tick(220);
  await vi.waitFor(() => expect(player.snapshot().current).toBeNull());
  expect(player.snapshot().playing).toBe(false);
});

test("a later duration revision is reconciled against the server duration", async () => {
  const { player, media } = await start(200);
  expect(player.positionSnapshot().duration).toBe(200);
  media.duration = 600;
  media.dispatchEvent(new Event("durationchange"));
  expect(player.positionSnapshot().duration).toBe(220);
  media.tick(220);
  await vi.waitFor(() => expect(player.snapshot().current?.id).toBe("next"));
});

test("an unknown server duration leaves the browser duration and no end guard", async () => {
  const { player, media } = await start(440, [track("first"), timed("next")]);
  expect(player.positionSnapshot().duration).toBe(440);
  media.tick(300);
  await vi.advanceTimersByTimeAsync(10_000);
  expect(player.snapshot().current?.id).toBe("first");
});

test("prepared media applies the server duration and end guard after handoff", async () => {
  const initial = new Media();
  initial.duration = 200;
  const prepared = new Media();
  prepared.duration = 440;
  let created!: () => void;
  const ready = new Promise<void>((resolve) => {
    created = resolve;
  });
  const owner: BrowserMediaOwner = {
    create() {
      created();
      return prepared;
    },
    activate: vi.fn(),
    release: vi.fn(),
  };
  const player = new Playback(
    initial,
    async (id) => [{ id: `source-${id}`, ready: true }],
    undefined,
    owner,
  );
  await player.playQueue([timed("first"), timed("next")]);
  initial.metadata();
  await ready;
  prepared.metadata();
  prepared.dispatchEvent(new Event("canplay"));
  await player.next();
  expect(owner.activate).toHaveBeenCalledWith(prepared);
  expect(player.positionSnapshot().duration).toBe(220);
  prepared.tick(220);
  await vi.waitFor(() => expect(player.snapshot().current).toBeNull());
  player.retire();
});
