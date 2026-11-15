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
  media.ready(Number.NaN);
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

const clock: NonNullable<Parameters<typeof vi.useFakeTimers>[0]>["toFake"] = [
  "setTimeout",
  "clearTimeout",
  "setInterval",
  "clearInterval",
  "Date",
  "performance",
];

function heldHead(options: { obeyAbort: boolean }) {
  const requests: AbortSignal[] = [];
  let answer: ((response: Response) => void) | null = null;
  vi.stubGlobal(
    "fetch",
    vi.fn(
      (input: RequestInfo | URL, init?: RequestInit) =>
        new Promise<Response>((resolve, reject) => {
          const signal = init?.signal ?? new AbortController().signal;
          requests.push(signal);
          answer = () => resolve(convertedHead(input));
          if (options.obeyAbort)
            signal.addEventListener(
              "abort",
              () => reject(new DOMException("Aborted", "AbortError")),
              { once: true },
            );
        }),
    ),
  );
  return {
    requests,
    answer: () => answer?.(new Response(null)),
  };
}

const original = "/api/v1/sources/source/audio";

test("during the first search a held quality check gives way to Original at thirteen seconds and heard Original keeps playing", async () => {
  vi.useFakeTimers({ toFake: clock });
  localStorage.setItem("hito:stream-quality", "high");
  const head = heldHead({ obeyAbort: true });
  const media = new StreamMedia();
  const player = new Playback(media, async () => [
    { id: "source", ready: true },
  ]);
  try {
    const opening = player.playQueue([streamTrack], 0, {
      kind: "position",
      position: 12,
    });
    await vi.advanceTimersByTimeAsync(12_999);
    expect(head.requests).toHaveLength(1);
    expect(head.requests[0].aborted).toBe(false);
    expect(media.src).toBe("");
    await vi.advanceTimersByTimeAsync(1);
    expect(head.requests[0].aborted).toBe(true);
    expect(media.src).toBe(original);
    await opening;
    media.ready();
    expect(media.currentTime).toBe(12);
    expect(player.positionSnapshot()).toEqual({ position: 12, duration: 90 });
    media.advance(12.5);
    expect(player.snapshot().desiredPlaying).toBeUndefined();
    await vi.advanceTimersByTimeAsync(5_000);
    expect(player.snapshot().current?.id).toBe(streamTrack.id);
    expect(player.snapshot().playing).toBe(true);
    expect(media.src).toBe(original);
    expect(head.requests).toHaveLength(1);
  } finally {
    player.retire();
    vi.useRealTimers();
  }
});

test("a slow lookup shortens the quality check on the same clock and unheard Original still advances once at fifteen seconds", async () => {
  vi.useFakeTimers({ toFake: clock });
  localStorage.setItem("hito:stream-quality", "high");
  const head = heldHead({ obeyAbort: true });
  const media = new StreamMedia();
  const lookups: string[] = [];
  const player = new Playback(media, async (recording) => {
    lookups.push(recording);
    if (recording === streamTrack.id)
      await new Promise((resolve) => setTimeout(resolve, 5_000));
    return [
      { id: recording === streamTrack.id ? "source" : "next", ready: true },
    ];
  });
  try {
    void player.playQueue([streamTrack, { ...streamTrack, id: "next" }]);
    await vi.advanceTimersByTimeAsync(5_000);
    expect(head.requests).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(7_999);
    expect(head.requests[0].aborted).toBe(false);
    expect(media.src).toBe("");
    await vi.advanceTimersByTimeAsync(1);
    expect(head.requests[0].aborted).toBe(true);
    expect(media.src).toBe(original);
    media.ready();
    await vi.advanceTimersByTimeAsync(1_999);
    expect(player.snapshot().current?.id).toBe(streamTrack.id);
    await vi.advanceTimersByTimeAsync(1);
    expect(player.snapshot().current?.id).toBe("next");
    expect(player.snapshot().index).toBe(1);
    await vi.advanceTimersByTimeAsync(30_000);
    expect(
      lookups.filter((recording) => recording === streamTrack.id).length,
    ).toBe(1);
    expect(player.snapshot().index).toBe(1);
  } finally {
    player.retire();
    vi.useRealTimers();
  }
});

test("with two seconds or less left the quality check is skipped and Original opens at once", async () => {
  vi.useFakeTimers({ toFake: clock });
  localStorage.setItem("hito:stream-quality", "high");
  const head = heldHead({ obeyAbort: true });
  const media = new StreamMedia();
  const player = new Playback(media, async () => {
    await new Promise((resolve) => setTimeout(resolve, 13_000));
    return [{ id: "source", ready: true }];
  });
  try {
    void player.playQueue([streamTrack], 0, { kind: "position", position: 12 });
    await vi.advanceTimersByTimeAsync(13_000);
    expect(head.requests).toHaveLength(0);
    expect(media.src).toBe(original);
    media.ready();
    expect(media.currentTime).toBe(12);
  } finally {
    player.retire();
    vi.useRealTimers();
  }
});

test("a quality answer that arrives after Original was chosen cannot replace it or change the position", async () => {
  vi.useFakeTimers({ toFake: clock });
  localStorage.setItem("hito:stream-quality", "high");
  const head = heldHead({ obeyAbort: false });
  const media = new StreamMedia();
  const player = new Playback(media, async () => [
    { id: "source", ready: true },
  ]);
  try {
    void player.playQueue([streamTrack], 0, { kind: "position", position: 12 });
    await vi.advanceTimersByTimeAsync(13_000);
    expect(media.src).toBe(original);
    media.ready();
    media.advance(12.5);
    const before = player.snapshot();
    head.answer();
    await vi.advanceTimersByTimeAsync(5_000);
    expect(media.src).toBe(original);
    expect(player.snapshot()).toBe(before);
    expect(player.positionSnapshot().position).toBe(12.5);
    expect(head.requests).toHaveLength(1);
  } finally {
    player.retire();
    vi.useRealTimers();
  }
});

test("a seek before converted audio is heard renegotiates within the same clock and heard Original ends the search", async () => {
  vi.useFakeTimers({ toFake: clock });
  localStorage.setItem("hito:stream-quality", "high");
  let hold = false;
  const signals: AbortSignal[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      if (!hold) return Promise.resolve(convertedHead(input));
      return new Promise<Response>((_resolve, reject) => {
        const signal = init!.signal!;
        signals.push(signal);
        signal.addEventListener(
          "abort",
          () => reject(new DOMException("Aborted", "AbortError")),
          { once: true },
        );
      });
    }),
  );
  const media = new StreamMedia();
  const player = new Playback(media, async () => [
    { id: "source", ready: true },
  ]);
  try {
    void player.playQueue([streamTrack]);
    await vi.advanceTimersByTimeAsync(0);
    expect(media.src).toContain("quality=high");
    media.ready(Number.NaN);
    expect(player.snapshot().desiredPlaying).toBe(true);
    await vi.advanceTimersByTimeAsync(5_000);
    hold = true;
    player.seek(40);
    await vi.advanceTimersByTimeAsync(7_999);
    expect(signals).toHaveLength(1);
    expect(signals[0].aborted).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    expect(signals[0].aborted).toBe(true);
    expect(media.src).toBe(original);
    media.ready();
    expect(media.currentTime).toBe(40);
    media.advance(40);
    expect(player.snapshot().desiredPlaying).toBe(true);
    media.advance(40.5);
    expect(player.snapshot().desiredPlaying).toBeUndefined();
    await vi.advanceTimersByTimeAsync(5_000);
    expect(player.snapshot().current?.id).toBe(streamTrack.id);
    expect(player.snapshot().playing).toBe(true);
    expect(media.src).toBe(original);
    expect(signals).toHaveLength(1);
  } finally {
    player.retire();
    vi.useRealTimers();
  }
});

test("after a blocked progress save and Play, a converted seek uses the fresh search clock and Original keeps its reserve", async () => {
  vi.useFakeTimers({ toFake: clock });
  localStorage.setItem("hito:stream-quality", "high");
  let hold = false;
  const signals: AbortSignal[] = [];
  const held: string[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      if (!hold) return Promise.resolve(convertedHead(input));
      return new Promise<Response>((_resolve, reject) => {
        const signal = init!.signal!;
        signals.push(signal);
        held.push(new URL(String(input), "http://hito.invalid").pathname);
        signal.addEventListener(
          "abort",
          () => reject(new DOMException("Aborted", "AbortError")),
          { once: true },
        );
      });
    }),
  );
  const media = new StreamMedia();
  let admit = false;
  const player = new Playback(
    media,
    async (recording) => [
      { id: recording === streamTrack.id ? "source" : "next", ready: true },
    ],
    () => admit,
  );
  try {
    void player.playQueue([streamTrack, { ...streamTrack, id: "next" }], 0, {
      kind: "position",
      position: 12,
    });
    await vi.advanceTimersByTimeAsync(0);
    expect(media.src).toContain("start_ms=12000");
    media.ready(Number.NaN);
    media.advance(0);
    expect(player.snapshot().error).toBe(
      "Listening progress is waiting to be saved. Playback paused.",
    );
    expect(player.snapshot().desiredPlaying).toBeUndefined();
    await vi.advanceTimersByTimeAsync(5_000);
    admit = true;
    await player.setPlaying(true);
    expect(player.snapshot().desiredPlaying).toBe(true);
    await vi.advanceTimersByTimeAsync(5_000);
    hold = true;
    player.seek(40);
    await vi.advanceTimersByTimeAsync(7_999);
    expect(signals).toHaveLength(1);
    expect(signals[0].aborted).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    expect(signals[0].aborted).toBe(true);
    expect(media.src).toBe(original);
    media.ready();
    media.advance(40);
    expect(player.snapshot().desiredPlaying).toBe(true);
    await vi.advanceTimersByTimeAsync(1_999);
    expect(player.snapshot().current?.id).toBe(streamTrack.id);
    expect(held).toEqual(["/api/v1/sources/source/audio"]);
    await vi.advanceTimersByTimeAsync(1);
    expect(player.snapshot().current?.id).toBe("next");
    expect(held).toEqual([
      "/api/v1/sources/source/audio",
      "/api/v1/sources/next/audio",
    ]);
  } finally {
    player.retire();
    vi.useRealTimers();
  }
});

test("after audio is heard a seek keeps the ordinary fifteen second quality check before Original", async () => {
  vi.useFakeTimers({ toFake: clock });
  localStorage.setItem("hito:stream-quality", "high");
  let hold = false;
  const signals: AbortSignal[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      if (!hold) return Promise.resolve(convertedHead(input));
      return new Promise<Response>((_resolve, reject) => {
        const signal = init!.signal!;
        signals.push(signal);
        signal.addEventListener(
          "abort",
          () => reject(new DOMException("Aborted", "AbortError")),
          { once: true },
        );
      });
    }),
  );
  const media = new StreamMedia();
  const player = new Playback(media, async () => [
    { id: "source", ready: true },
  ]);
  try {
    void player.playQueue([streamTrack]);
    await vi.advanceTimersByTimeAsync(0);
    expect(media.src).toContain("quality=high");
    media.ready(Number.NaN);
    media.advance(1);
    expect(player.snapshot().desiredPlaying).toBeUndefined();
    hold = true;
    player.seek(40);
    await vi.advanceTimersByTimeAsync(14_999);
    expect(signals).toHaveLength(1);
    expect(signals[0].aborted).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    expect(signals[0].aborted).toBe(true);
    expect(media.src).toBe(original);
    expect(player.snapshot().current?.id).toBe(streamTrack.id);
  } finally {
    player.retire();
    vi.useRealTimers();
  }
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
