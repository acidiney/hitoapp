import { expect, test, vi } from "vitest";
import { Playback } from "../playback";
import {
  StreamMedia,
  streamTrack,
  convertedHead,
} from "../fixtures/browserStreamMedia";

test("a superseded quality request is aborted and its late answer cannot replace the new selection", async () => {
  localStorage.setItem("hito:stream-quality", "high");
  let release!: () => void;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  let signal: AbortSignal | null = null;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, options?: RequestInit) => {
      if (String(input).includes("/old-source/")) {
        signal = options?.signal ?? null;
        await held;
      }
      return convertedHead(input);
    }),
  );
  const media = new StreamMedia();
  const player = new Playback(media, async (recording) => [
    { id: `${recording}-source`, ready: true },
  ]);
  const old = player.playQueue([{ ...streamTrack, id: "old" }]);
  await vi.waitFor(() => expect(signal).not.toBeNull());
  await player.playQueue([{ ...streamTrack, id: "new" }]);
  expect(signal!.aborted).toBe(true);
  media.ready();
  const currentUrl = media.src;
  release();
  await old;
  expect(media.src).toBe(currentUrl);
  expect(player.snapshot().current?.id).toBe("new");
  expect(player.snapshot().error).toBe("");
  player.retire();
});

test("retiring browser playback aborts its quality request and never starts late audio", async () => {
  localStorage.setItem("hito:stream-quality", "saver");
  let release!: () => void;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  let signal: AbortSignal | null = null;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, options?: RequestInit) => {
      signal = options?.signal ?? null;
      await held;
      return convertedHead(input);
    }),
  );
  const media = new StreamMedia();
  const player = new Playback(media, async () => [
    { id: "source", ready: true },
  ]);
  const opening = player.playQueue([streamTrack]);
  await vi.waitFor(() => expect(signal).not.toBeNull());
  player.retire();
  expect(signal!.aborted).toBe(true);
  release();
  await opening;
  expect(media.src).toBe("");
  expect(media.paused).toBe(true);
  expect(player.snapshot().current).toBeNull();
});

test.each(["high", "saver"])(
  "%s preserves original media timing when negotiation keeps a remote or small source unchanged",
  async (quality) => {
    localStorage.setItem("hito:stream-quality", quality);
    const request = vi.fn(
      async () =>
        new Response(null, {
          headers: { "X-Hito-Stream-Quality": "original" },
        }),
    );
    vi.stubGlobal("fetch", request);
    const media = new StreamMedia();
    const player = new Playback(media, async () => [
      { id: "source", ready: true },
    ]);
    await player.playQueue([streamTrack], 0, {
      kind: "position",
      position: 12,
    });
    media.ready();
    expect(media.src).toBe("/api/v1/sources/source/audio");
    expect(media.currentTime).toBe(12);
    player.seek(40);
    expect(media.currentTime).toBe(40);
    expect(player.positionSnapshot()).toEqual({ position: 40, duration: 90 });
    expect(request).toHaveBeenCalledTimes(1);
    player.retire();
  },
);

test("a converted audiobook preserves absolute heard checkpoints and does not save a silent paused seek", async () => {
  localStorage.setItem("hito:stream-quality", "high");
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL) => convertedHead(input)),
  );
  const media = new StreamMedia();
  const checkpoints: { position: number; chapter: unknown }[] = [];
  const player = new Playback(
    media,
    async () => [{ id: "source", ready: true }],
    (_track, position, _speed, chapter) => {
      checkpoints.push({ position, chapter });
    },
  );
  const chapter = {
    audiobook_id: "book",
    entry_id: "entry",
    recording_id: streamTrack.id,
  };
  await player.playQueue([{ recording: streamTrack, chapter }], 0, {
    kind: "position",
    position: 12,
  });
  media.ready(Number.NaN);
  await player.setPlaying(false);
  expect(checkpoints).toEqual([]);
  await player.setPlaying(true);
  media.advance(1);
  await player.setPlaying(false);
  expect(checkpoints.at(-1)).toEqual({ position: 13, chapter });
  const count = checkpoints.length;
  player.seek(40);
  await vi.waitFor(() => expect(media.src).toContain("start_ms=40000"));
  media.ready(Number.NaN);
  expect(checkpoints).toHaveLength(count);
  expect(media.paused).toBe(true);
  await player.setPlaying(true);
  media.advance(2);
  await player.setPlaying(false);
  expect(checkpoints.at(-1)).toEqual({ position: 42, chapter });
  expect(player.positionSnapshot().duration).toBe(90);
  player.retire();
});

test("a quality handshake that reaches its deadline aborts and opens Original at the saved position", async () => {
  vi.useFakeTimers();
  localStorage.setItem("hito:stream-quality", "high");
  let signal: AbortSignal | null = null;
  vi.stubGlobal(
    "fetch",
    vi.fn(
      (_input: RequestInfo | URL, options?: RequestInit) =>
        new Promise<Response>((_resolve, reject) => {
          signal = options?.signal ?? null;
          signal?.addEventListener(
            "abort",
            () => reject(new DOMException("Aborted", "AbortError")),
            { once: true },
          );
        }),
    ),
  );
  const media = new StreamMedia();
  const player = new Playback(media, async () => [
    { id: "source", ready: true },
  ]);
  const opening = player.playQueue([streamTrack], 0, {
    kind: "position",
    position: 12,
  });
  await vi.advanceTimersByTimeAsync(15_000);
  await opening;
  expect(signal!.aborted).toBe(true);
  expect(media.src).toBe("/api/v1/sources/source/audio");
  media.ready();
  expect(media.currentTime).toBe(12);
  expect(player.positionSnapshot()).toEqual({ position: 12, duration: 90 });
  player.retire();
});

test("an Original fallback that rejects its resume seek recovers through another authorized source at the absolute position", async () => {
  localStorage.setItem("hito:stream-quality", "high");
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL) => convertedHead(input)),
  );
  const media = new StreamMedia();
  let position = 0;
  Object.defineProperty(media, "currentTime", {
    get: () => position,
    set: (value: number) => {
      if (media.src === "/api/v1/sources/source/audio" && value > 0)
        throw new DOMException("Owned seek refusal", "InvalidStateError");
      position = value;
    },
  });
  const player = new Playback(media, async (_recording, options) => [
    {
      id: options.purpose === "refresh" ? "replacement" : "source",
      ready: true,
    },
  ]);
  await player.playQueue([streamTrack], 0, { kind: "position", position: 12 });
  media.ready(78);
  media.advance(2);
  media.error = { code: 2 };
  media.dispatchEvent(new Event("error"));
  expect(media.src).toBe("/api/v1/sources/source/audio");
  media.ready();
  await vi.waitFor(() => expect(media.src).toContain("/replacement/audio"));
  media.ready(76);
  expect(player.positionSnapshot()).toEqual({ position: 14, duration: 90 });
  expect(player.snapshot().error).toBe("");
  player.retire();
});
