import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { Playback, type MediaPort } from "../playback";
import { getConnection, resetConnection } from "../../../shared/connection";

class Media extends EventTarget implements MediaPort {
  src = "";
  currentTime = 0;
  duration = 90;
  playbackRate = 1;
  defaultPlaybackRate = 1;
  volume = 1;
  paused = true;
  readyState = 0;
  loads: string[] = [];
  async play() {
    this.paused = false;
    this.dispatchEvent(new Event("play"));
  }
  pause() {
    if (this.paused) return;
    this.paused = true;
    this.dispatchEvent(new Event("pause"));
  }
  load() {
    this.loads.push(this.src);
    this.currentTime = 0;
    this.readyState = 0;
  }
  removeAttribute() {
    this.src = "";
  }
  ready() {
    this.readyState = 4;
    this.dispatchEvent(new Event("loadedmetadata"));
  }
  flow() {
    this.readyState = 4;
    this.dispatchEvent(new Event("playing"));
  }
  progress(seconds: number) {
    this.currentTime = seconds;
    this.dispatchEvent(new Event("timeupdate"));
  }
  starve() {
    this.readyState = 2;
    this.dispatchEvent(new Event("waiting"));
  }
}

const track = (id: string) => ({
  id,
  title: `Title of ${id}`,
  artist: "Fixture",
  duration_ms: 90000,
  available: true,
});

const UNREACHABLE = "Hito can't reach your server. Press play to try again.";

let serverAnswers = true;

beforeEach(() => {
  serverAnswers = true;
  vi.useFakeTimers({
    toFake: [
      "setTimeout",
      "clearTimeout",
      "setInterval",
      "clearInterval",
      "Date",
      "performance",
    ],
  });
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL) => {
      if (String(input) !== "/health")
        throw new Error("Only the server health check is expected");
      if (!serverAnswers) throw new TypeError("Failed to fetch");
      return new Response("ok");
    }),
  );
});

afterEach(() => {
  resetConnection();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

async function playing(
  sources: Record<string, string[]> = { first: ["a"] },
  queue = ["first"],
) {
  const media = new Media();
  const requests: string[] = [];
  const player = new Playback(media, async (id, options) => {
    requests.push(`${id}:${options.purpose}`);
    return options.purpose === "cached"
      ? (sources[id] ?? []).map((source) => ({ id: source, ready: true }))
      : [];
  });
  await player.playQueue(queue.map(track));
  media.ready();
  media.flow();
  media.progress(30);
  return { media, player, requests };
}

test("a playing track that runs out of audio shows Buffering until audio flows again", async () => {
  const { media, player } = await playing();
  expect(player.snapshot().buffering ?? false).toBe(false);

  media.starve();
  expect(player.snapshot()).toMatchObject({
    buffering: true,
    playing: true,
    position: 30,
  });

  media.flow();
  expect(player.snapshot()).toMatchObject({ buffering: false, playing: true });

  media.starve();
  await player.toggle();
  expect(player.snapshot()).toMatchObject({ buffering: false, playing: false });
});

test("a stall that lasts fifteen seconds reopens the same source at the same position", async () => {
  const { media, player, requests } = await playing();
  const loads = media.loads.length;
  const lookups = [...requests];

  media.starve();
  await vi.advanceTimersByTimeAsync(14_999);
  expect(media.loads).toHaveLength(loads);

  await vi.advanceTimersByTimeAsync(1);
  expect(media.loads).toHaveLength(loads + 1);
  expect(media.loads.at(-1)).toContain("/a/audio");
  expect(requests).toEqual(lookups);
  expect(player.snapshot()).toMatchObject({
    buffering: true,
    current: { id: "first" },
  });

  media.ready();
  expect(media.currentTime).toBe(30);
  media.flow();
  expect(player.snapshot()).toMatchObject({
    buffering: false,
    playing: true,
    current: { id: "first" },
    error: "",
  });
});

test("a stall that outlasts the rebuild moves to the next source at the same position, then to the next track", async () => {
  const { media, player } = await playing(
    { first: ["a", "b"], second: ["c"] },
    ["first", "second"],
  );

  media.starve();
  await vi.advanceTimersByTimeAsync(15_000);
  expect(media.loads.at(-1)).toContain("/a/audio");
  media.ready();

  await vi.advanceTimersByTimeAsync(15_000);
  expect(media.loads.at(-1)).toContain("/b/audio");
  expect(player.snapshot().current?.id).toBe("first");
  media.ready();
  expect(media.currentTime).toBe(30);
  media.flow();
  media.progress(31);
  expect(player.snapshot()).toMatchObject({ buffering: false, playing: true });

  media.starve();
  await vi.advanceTimersByTimeAsync(15_000);
  expect(media.loads.at(-1)).toContain("/b/audio");
  media.ready();
  await vi.advanceTimersByTimeAsync(15_000);

  expect(media.loads.at(-1)).toContain("/c/audio");
  expect(player.snapshot()).toMatchObject({
    current: { id: "second" },
    buffering: false,
    error: "Audio is unavailable for Title of first.",
  });
});

test("an unreachable server keeps the track waiting instead of skipping, then pauses where it stopped", async () => {
  const { media, player, requests } = await playing(
    { first: ["a"], second: ["c"] },
    ["first", "second"],
  );
  const loads = media.loads.length;
  const lookups = [...requests];
  serverAnswers = false;

  media.starve();
  await vi.advanceTimersByTimeAsync(15_000 * 8);
  expect(player.snapshot()).toMatchObject({
    current: { id: "first" },
    buffering: true,
    playing: true,
    error: "",
  });
  expect(getConnection().reachable).toBe(false);
  expect(media.loads).toHaveLength(loads);
  expect(requests).toEqual(lookups);

  await vi.advanceTimersByTimeAsync(15_000);
  expect(player.snapshot()).toMatchObject({
    current: { id: "first" },
    buffering: false,
    playing: false,
    position: 30,
    error: UNREACHABLE,
  });
  expect(media.paused).toBe(true);
  expect(requests).toEqual(lookups);

  serverAnswers = true;
  await player.toggle();
  media.ready();
  expect(media.loads.at(-1)).toContain("/a/audio");
  expect(media.currentTime).toBe(30);
  media.flow();
  expect(player.snapshot()).toMatchObject({
    current: { id: "first" },
    playing: true,
    error: "",
  });
});

test("a media error while the server cannot be reached keeps the track and reopens it where it stopped once the server answers", async () => {
  const { media, player, requests } = await playing(
    { first: ["a"], second: ["c"] },
    ["first", "second"],
  );
  const loads = media.loads.length;
  const lookups = [...requests];
  serverAnswers = false;

  media.dispatchEvent(new Event("error"));
  await vi.advanceTimersByTimeAsync(0);
  expect(player.snapshot()).toMatchObject({
    current: { id: "first" },
    buffering: true,
    error: "",
  });
  expect(requests).toEqual(lookups);
  expect(media.loads).toHaveLength(loads);

  serverAnswers = true;
  await vi.advanceTimersByTimeAsync(15_000);
  expect(media.loads).toHaveLength(loads + 1);
  expect(media.loads.at(-1)).toContain("/a/audio");
  expect(requests).toEqual(lookups);
  media.ready();
  expect(media.currentTime).toBe(30);
  expect(player.snapshot().current?.id).toBe("first");
});

test("a server that answers again is reopened at the next deadline", async () => {
  const { media, player } = await playing();
  const loads = media.loads.length;
  serverAnswers = false;

  media.starve();
  await vi.advanceTimersByTimeAsync(15_000);
  expect(media.loads).toHaveLength(loads);

  serverAnswers = true;
  await vi.advanceTimersByTimeAsync(15_000);
  expect(media.loads).toHaveLength(loads + 1);
  expect(media.loads.at(-1)).toContain("/a/audio");
  media.ready();
  expect(media.currentTime).toBe(30);
  expect(player.snapshot().current?.id).toBe("first");
});

test("a media error that also pauses the element still moves to the next source at the same position", async () => {
  const { media, player } = await playing({ first: ["a", "b"] });

  media.dispatchEvent(new Event("error"));
  media.pause();
  await vi.advanceTimersByTimeAsync(0);

  expect(media.loads.at(-1)).toContain("/b/audio");
  media.ready();
  expect(media.currentTime).toBe(30);
  expect(player.snapshot().current?.id).toBe("first");
});
