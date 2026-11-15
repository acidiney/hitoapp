import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { Schema } from "../../../shared/schema";
import { HttpError } from "../../../shared/http";
import { log } from "../../../shared/log";
import { Playback, type MediaPort } from "../playback";
import type { Source, SourceResolver } from "../api/playbackSources";
import { BrowserStream } from "../browser/BrowserStream";
import {
  audioPath,
  browserPreparationModes,
  recording,
} from "../fixtures/browserPreparationModes";

class Media extends EventTarget implements MediaPort {
  src = "";
  currentTime = 0;
  duration = 90;
  playbackRate = 1;
  defaultPlaybackRate = 1;
  volume = 1;
  paused = true;
  seeking = false;
  readyState = 4;
  error: { code: number } | null = null;
  plays: (() => Promise<void>) | null = null;
  async play() {
    if (this.plays) await this.plays();
    this.paused = false;
    this.dispatchEvent(new Event("play"));
  }
  pause() {
    if (this.paused) return;
    this.paused = true;
    this.dispatchEvent(new Event("pause"));
  }
  load() {
    this.currentTime = 0;
  }
  removeAttribute() {
    this.src = "";
  }
  advance(seconds: number) {
    this.currentTime = seconds;
    this.dispatchEvent(new Event("timeupdate"));
  }
}

type Outcome = Schema<"SourceResolution">["outcome"];
type Call = { recording: string; purpose: string; request?: string };

const track = (id: string) => ({
  id,
  title: `${id} private title canary`,
  artist: "Fixture",
  duration_ms: 90000,
  available: true,
});

function resolution(
  recording: string,
  request: string,
  outcome: Outcome,
  source?: string,
): Schema<"SourceResolution"> {
  return {
    request_id: request,
    resolution_id: "fcff73a7-3a51-495e-a666-6de3460edd8c",
    recording_id: recording,
    outcome,
    sources: source
      ? [
          {
            id: source,
            recording_id: recording,
            provider: "plugin",
            device_id: null,
            ready: true,
            label: "Owned audio service",
            can_prepare: false,
            match_kind: "verified",
          },
        ]
      : [],
  };
}

function discovery(
  answer: (
    recording: string,
    pass: number,
    request: string,
    signal: AbortSignal,
  ) => Outcome | "ready" | Promise<Schema<"SourceResolution">>,
) {
  const calls: Call[] = [];
  const signals: AbortSignal[] = [];
  const passes = new Map<string, number>();
  const resolver: SourceResolver = async (recording, options) => {
    signals.push(options.signal);
    if (options.purpose !== "resolve") {
      calls.push({ recording, purpose: options.purpose });
      return [];
    }
    const request = options.body.request_id;
    calls.push({ recording, purpose: "resolve", request });
    const pass = (passes.get(recording) ?? 0) + 1;
    passes.set(recording, pass);
    const outcome = answer(recording, pass, request, options.signal);
    if (typeof outcome !== "string") return outcome;
    return outcome === "ready"
      ? resolution(recording, request, "ready", `${recording}-audio`)
      : resolution(recording, request, outcome);
  };
  const resolves = (recording: string) =>
    calls.filter(
      (call) => call.recording === recording && call.purpose === "resolve",
    );
  return { resolver, calls, signals, resolves };
}

const pass = (recording: string) => [
  { recording, purpose: "cached" },
  { recording, purpose: "refresh" },
  { recording, purpose: "resolve" },
];

const protocol = (calls: Call[]) =>
  calls.map(({ recording, purpose }) => ({ recording, purpose }));

let logged: unknown[][];

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  logged = [];
  for (const level of ["debug", "info", "warn", "error"] as const)
    vi.spyOn(log, level).mockImplementation((...values: unknown[]) => {
      logged.push(values);
    });
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  expect(JSON.stringify(logged)).not.toContain("private title canary");
});

test("two unavailable passes retry the same occurrence and its status clears only when the third pass is heard", async () => {
  const media = new Media();
  const { resolver, calls, resolves } = discovery((_recording, pass) =>
    pass === 1
      ? "provider_unavailable"
      : pass === 2
        ? "no_suitable_source"
        : "ready",
  );
  const player = new Playback(media, resolver);
  void player.playQueue([track("first"), track("second")]);
  await vi.advanceTimersByTimeAsync(0);
  const before = await player.queuePage(0, 10);
  expect(player.snapshot().retrying).toEqual({ attempt: 2, of: 3 });
  expect(player.snapshot().current?.id).toBe("first");
  expect(player.snapshot().error).toBe("");
  expect(protocol(calls)).toEqual(pass("first"));
  await vi.advanceTimersByTimeAsync(499);
  expect(calls).toHaveLength(3);
  await vi.advanceTimersByTimeAsync(1);
  expect(protocol(calls)).toEqual([...pass("first"), ...pass("first")]);
  expect(player.snapshot().retrying).toEqual({ attempt: 3, of: 3 });
  await vi.advanceTimersByTimeAsync(999);
  expect(calls).toHaveLength(6);
  await vi.advanceTimersByTimeAsync(1);
  expect(protocol(calls)).toEqual([
    ...pass("first"),
    ...pass("first"),
    ...pass("first"),
  ]);
  const requests = resolves("first").map((call) => call.request);
  expect(new Set(requests).size).toBe(3);
  expect(media.src).toContain("/first-audio/audio");
  expect(player.snapshot().playing).toBe(true);
  expect(player.snapshot().retrying).toEqual({ attempt: 3, of: 3 });
  media.dispatchEvent(new Event("loadedmetadata"));
  media.dispatchEvent(new Event("canplay"));
  media.advance(0);
  expect(player.snapshot().retrying).toEqual({ attempt: 3, of: 3 });
  media.advance(0.25);
  expect(player.snapshot().retrying ?? null).toBeNull();
  expect(player.snapshot().desiredPlaying).not.toBe(false);
  expect(player.snapshot().current?.id).toBe("first");
  expect(player.snapshot().index).toBe(0);
  expect(player.snapshot().error).toBe("");
  expect(await player.queuePage(0, 10)).toEqual(before);
  await vi.advanceTimersByTimeAsync(20000);
  expect(player.snapshot().current?.id).toBe("first");
});

test("three exhausted passes advance once to the next occurrence and never start a fourth", async () => {
  const media = new Media();
  const { resolver, resolves } = discovery((recording) =>
    recording === "first" ? "provider_unavailable" : "ready",
  );
  const player = new Playback(media, resolver);
  void player.playQueue([track("first"), track("second"), track("third")]);
  await vi.advanceTimersByTimeAsync(1499);
  expect(player.snapshot().current?.id).toBe("first");
  expect(resolves("first")).toHaveLength(2);
  await vi.advanceTimersByTimeAsync(1);
  expect(resolves("first")).toHaveLength(3);
  expect(player.snapshot().current?.id).toBe("second");
  expect(player.snapshot().index).toBe(1);
  expect(player.snapshot().error).toBe(
    "An audio service is unavailable. Select the track again to retry.",
  );
  expect(player.snapshot().retrying ?? null).toBeNull();
  expect(media.src).toContain("/second-audio/audio");
  await vi.advanceTimersByTimeAsync(5000);
  expect(resolves("first")).toHaveLength(3);
  expect(resolves("third")).toHaveLength(0);
});

test("a held retry grant and play promise keep the retry status until the media actually advances", async () => {
  const media = new Media();
  let release!: () => void;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  const { resolver } = discovery((_recording, pass) =>
    pass === 1 ? "no_suitable_source" : "ready",
  );
  const player = new Playback(media, resolver);
  void player.playQueue([track("first"), track("second")]);
  await vi.advanceTimersByTimeAsync(0);
  media.plays = () => held;
  await vi.advanceTimersByTimeAsync(500);
  expect(media.src).toContain("/first-audio/audio");
  expect(player.snapshot().retrying).toEqual({ attempt: 2, of: 3 });
  release();
  await vi.advanceTimersByTimeAsync(0);
  media.dispatchEvent(new Event("loadedmetadata"));
  expect(player.snapshot().playing).toBe(true);
  expect(player.snapshot().retrying).toEqual({ attempt: 2, of: 3 });
  media.seeking = true;
  media.advance(1);
  expect(player.snapshot().retrying).toEqual({ attempt: 2, of: 3 });
  media.seeking = false;
  media.advance(1.5);
  expect(player.snapshot().retrying ?? null).toBeNull();
});

test("a lookup that never settles ends at the original fifteen seconds and advances once", async () => {
  const media = new Media();
  const { resolver, signals, resolves } = discovery(
    (recording, _pass, _request, signal) =>
      recording === "first"
        ? new Promise((_resolve, reject) =>
            signal.addEventListener("abort", () =>
              reject(new DOMException("Aborted", "AbortError")),
            ),
          )
        : "ready",
  );
  const player = new Playback(media, resolver);
  void player.playQueue([track("first"), track("second")]);
  await vi.advanceTimersByTimeAsync(14999);
  expect(player.snapshot().current?.id).toBe("first");
  expect(player.snapshot().loading).toBe(true);
  expect(resolves("first")).toHaveLength(1);
  const pending = [...signals];
  await vi.advanceTimersByTimeAsync(1);
  expect(pending.every((signal) => signal.aborted)).toBe(true);
  expect(player.snapshot().current?.id).toBe("second");
  expect(player.snapshot().error).toBe(
    "Audio is unavailable for first private title canary.",
  );
  expect(resolves("first")).toHaveLength(1);
  expect(media.src).toContain("/second-audio/audio");
});

test("granted audio that never moves ends at the original fifteen seconds even after a retry", async () => {
  const media = new Media();
  const { resolver, resolves } = discovery((recording, pass) =>
    recording === "first" && pass === 1 ? "provider_unavailable" : "ready",
  );
  const player = new Playback(media, resolver);
  void player.playQueue([track("first"), track("second")]);
  await vi.advanceTimersByTimeAsync(500);
  expect(media.src).toContain("/first-audio/audio");
  media.dispatchEvent(new Event("loadedmetadata"));
  media.advance(0);
  await vi.advanceTimersByTimeAsync(14499);
  expect(player.snapshot().current?.id).toBe("first");
  expect(player.snapshot().retrying).toEqual({ attempt: 2, of: 3 });
  await vi.advanceTimersByTimeAsync(1);
  expect(player.snapshot().current?.id).toBe("second");
  expect(player.snapshot().retrying ?? null).toBeNull();
  expect(resolves("first")).toHaveLength(2);
  expect(media.src).toContain("/second-audio/audio");
});

test("pausing during a held retry stops it, keeps the occurrence selected and Play starts a fresh bounded search", async () => {
  const media = new Media();
  const { resolver, calls, signals } = discovery((_recording, pass) =>
    pass === 1 ? "provider_unavailable" : "ready",
  );
  const player = new Playback(media, resolver);
  void player.playQueue([track("first"), track("second")]);
  await vi.advanceTimersByTimeAsync(0);
  expect(player.snapshot().retrying).toEqual({ attempt: 2, of: 3 });
  expect(player.snapshot().desiredPlaying).toBe(true);
  await player.toggle();
  expect(player.snapshot().retrying ?? null).toBeNull();
  expect(player.snapshot().desiredPlaying).toBe(false);
  expect(player.snapshot().loading).toBe(false);
  expect(player.snapshot().playing).toBe(false);
  expect(player.snapshot().current?.id).toBe("first");
  expect(player.snapshot().index).toBe(0);
  expect(signals.every((signal) => signal.aborted)).toBe(true);
  await vi.advanceTimersByTimeAsync(20000);
  expect(calls).toHaveLength(3);
  expect(player.snapshot().current?.id).toBe("first");
  expect(media.src).toBe("");
  void player.setPlaying(true);
  await vi.advanceTimersByTimeAsync(0);
  expect(protocol(calls)).toEqual([...pass("first"), ...pass("first")]);
  expect(media.src).toContain("/first-audio/audio");
  expect(player.snapshot().retrying ?? null).toBeNull();
});

test("an explicit Play while a search is in progress does not pause or duplicate it", async () => {
  const media = new Media();
  const { resolver, calls } = discovery(() => "provider_unavailable");
  const player = new Playback(media, resolver);
  void player.playQueue([track("first"), track("second")]);
  await vi.advanceTimersByTimeAsync(0);
  await player.setPlaying(true);
  expect(player.snapshot().retrying).toEqual({ attempt: 2, of: 3 });
  expect(player.snapshot().desiredPlaying).toBe(true);
  expect(calls).toHaveLength(3);
  await vi.advanceTimersByTimeAsync(500);
  expect(calls).toHaveLength(6);
});

test("Next during a held retry cancels it and its late reply cannot restore audio or status", async () => {
  const media = new Media();
  let finish!: (value: Schema<"SourceResolution">) => void;
  let held = "";
  const { resolver, resolves } = discovery((recording, pass, request) => {
    if (recording !== "first") return "ready";
    if (pass === 1) return "provider_unavailable";
    held = request;
    return new Promise((resolve) => {
      finish = resolve;
    });
  });
  const player = new Playback(media, resolver);
  void player.playQueue([track("first"), track("second")]);
  await vi.advanceTimersByTimeAsync(500);
  expect(resolves("first")).toHaveLength(2);
  await player.next();
  await vi.advanceTimersByTimeAsync(0);
  expect(player.snapshot().current?.id).toBe("second");
  expect(media.src).toContain("/second-audio/audio");
  media.advance(1);
  expect(player.snapshot().desiredPlaying).toBeUndefined();
  finish(resolution("first", held, "ready", "late-first-audio"));
  await vi.advanceTimersByTimeAsync(20000);
  expect(media.src).toContain("/second-audio/audio");
  expect(player.snapshot().retrying ?? null).toBeNull();
  expect(player.snapshot().current?.id).toBe("second");
  expect(resolves("first")).toHaveLength(2);
});

test("Previous during a held retry returns to the earlier occurrence and stops the retry", async () => {
  const media = new Media();
  const { resolver, resolves } = discovery((recording) =>
    recording === "second" ? "no_suitable_source" : "ready",
  );
  const player = new Playback(media, resolver);
  void player.playQueue([track("first"), track("second")], 1);
  await vi.advanceTimersByTimeAsync(0);
  expect(player.snapshot().retrying).toEqual({ attempt: 2, of: 3 });
  await player.previous();
  await vi.advanceTimersByTimeAsync(0);
  expect(player.snapshot().current?.id).toBe("first");
  expect(player.snapshot().index).toBe(0);
  expect(player.snapshot().retrying ?? null).toBeNull();
  expect(media.src).toContain("/first-audio/audio");
  media.advance(1);
  expect(player.snapshot().desiredPlaying).toBeUndefined();
  await vi.advanceTimersByTimeAsync(20000);
  expect(resolves("second")).toHaveLength(1);
  expect(player.snapshot().current?.id).toBe("first");
});

test("selecting another entry or clearing during a held retry leaves no work behind", async () => {
  const media = new Media();
  const { resolver, resolves, signals } = discovery((recording) =>
    recording === "first" ? "provider_unavailable" : "ready",
  );
  const player = new Playback(media, resolver);
  void player.playQueue([track("first"), track("second"), track("third")]);
  await vi.advanceTimersByTimeAsync(0);
  const page = await player.queuePage(0, 10);
  await player.selectEntry(page.entries[2].id);
  await vi.advanceTimersByTimeAsync(0);
  expect(player.snapshot().current?.id).toBe("third");
  expect(player.snapshot().retrying ?? null).toBeNull();
  await vi.advanceTimersByTimeAsync(20000);
  expect(resolves("first")).toHaveLength(1);
  expect(player.snapshot().current?.id).toBe("third");
  void player.playQueue([track("first")]);
  await vi.advanceTimersByTimeAsync(0);
  player.clear();
  await vi.advanceTimersByTimeAsync(20000);
  expect(resolves("first")).toHaveLength(2);
  expect(signals.every((signal) => signal.aborted)).toBe(true);
  expect(player.snapshot().retrying ?? null).toBeNull();
  expect(player.snapshot().current).toBeNull();
});

test("protected choices, cancelled searches and refused lookups skip without another pass", async () => {
  for (const failure of ["protected_choice", "cancelled", 403, 401] as const) {
    const media = new Media();
    const { resolver, resolves } = discovery((recording, _pass, request) => {
      if (recording !== "first") return "ready";
      if (typeof failure === "number")
        return Promise.reject(new HttpError(failure, "Refused"));
      return Promise.resolve(resolution("first", request, failure));
    });
    const player = new Playback(media, resolver);
    void player.playQueue([track("first"), track("second")]);
    await vi.advanceTimersByTimeAsync(0);
    expect(player.snapshot().current?.id).toBe("second");
    expect(player.snapshot().retrying ?? null).toBeNull();
    await vi.advanceTimersByTimeAsync(20000);
    expect(resolves("first")).toHaveLength(1);
    player.retire();
  }
});

test("a lost resolution reply is replayed with the same request inside one pass", async () => {
  const media = new Media();
  const requests: string[] = [];
  let failed = false;
  const player = new Playback(media, async (recording, options) => {
    if (options.purpose !== "resolve") return [];
    requests.push(options.body.request_id);
    if (!failed) {
      failed = true;
      throw new TypeError("Owned reply lost");
    }
    return resolution(recording, options.body.request_id, "ready", "audio");
  });
  void player.playQueue([track("first")]);
  await vi.advanceTimersByTimeAsync(0);
  expect(requests).toHaveLength(2);
  expect(requests[1]).toBe(requests[0]);
  expect(media.src).toContain("/audio/audio");
  expect(player.snapshot().retrying ?? null).toBeNull();
});

test("an exhausted lookup transport after its replay counts as one unavailable pass", async () => {
  const media = new Media();
  const requests: string[] = [];
  const player = new Playback(media, async (recording, options) => {
    if (options.purpose !== "resolve") return [];
    requests.push(options.body.request_id);
    if (requests.length <= 2) throw new TypeError("Owned network failure");
    return resolution(recording, options.body.request_id, "ready", "audio");
  });
  void player.playQueue([track("first")]);
  await vi.advanceTimersByTimeAsync(0);
  expect(requests).toHaveLength(2);
  expect(requests[1]).toBe(requests[0]);
  expect(player.snapshot().retrying).toEqual({ attempt: 2, of: 3 });
  await vi.advanceTimersByTimeAsync(500);
  expect(requests).toHaveLength(3);
  expect(requests[2]).not.toBe(requests[0]);
  expect(media.src).toContain("/audio/audio");
});

test("a blocked autoplay ends the search, keeps the selection and Play starts a fresh bounded search", async () => {
  const media = new Media();
  const { resolver, resolves } = discovery(() => "ready");
  const player = new Playback(media, resolver);
  media.plays = () =>
    Promise.reject(
      Object.assign(new Error("Blocked"), { name: "NotAllowedError" }),
    );
  void player.playQueue([track("first"), track("second")]);
  await vi.advanceTimersByTimeAsync(0);
  expect(player.snapshot().error).toBe("Press play to start audio.");
  expect(player.snapshot().desiredPlaying).toBe(false);
  expect(player.snapshot().loading).toBe(false);
  expect(player.snapshot().retrying ?? null).toBeNull();
  await vi.advanceTimersByTimeAsync(30000);
  expect(player.snapshot().current?.id).toBe("first");
  expect(player.snapshot().error).toBe("Press play to start audio.");
  expect(resolves("first")).toHaveLength(1);
  media.plays = null;
  void player.setPlaying(true);
  await vi.advanceTimersByTimeAsync(0);
  expect(resolves("first")).toHaveLength(2);
  expect(player.snapshot().playing).toBe(true);
  expect(player.snapshot().desiredPlaying).toBe(true);
  await vi.advanceTimersByTimeAsync(15000);
  expect(player.snapshot().current?.id).toBe("second");
});

test("retiring the player during a retry aborts its requests and timers", async () => {
  const media = new Media();
  const { resolver, calls, signals } = discovery(() => "provider_unavailable");
  const player = new Playback(media, resolver);
  void player.playQueue([track("first"), track("second")]);
  await vi.advanceTimersByTimeAsync(0);
  player.retire();
  await vi.advanceTimersByTimeAsync(20000);
  expect(calls).toHaveLength(3);
  expect(signals.every((signal) => signal.aborted)).toBe(true);
});

test("pausing granted audio whose play is still pending ignores the late play and Play starts a fresh bounded search", async () => {
  const media = new Media();
  let release!: () => void;
  media.plays = () =>
    new Promise<void>((resolve) => {
      release = resolve;
    });
  const { resolver, resolves } = discovery(() => "ready");
  const player = new Playback(media, resolver);
  void player.playQueue([track("first"), track("second")]);
  await vi.advanceTimersByTimeAsync(0);
  expect(media.src).toContain("/first-audio/audio");
  expect(player.snapshot().desiredPlaying).toBe(true);
  await player.toggle();
  expect(media.src).toBe("");
  expect(player.snapshot().desiredPlaying).toBe(false);
  release();
  await vi.advanceTimersByTimeAsync(0);
  expect(media.paused).toBe(true);
  expect(player.snapshot().playing).toBe(false);
  expect(player.snapshot().loading).toBe(false);
  expect(player.snapshot().retrying ?? null).toBeNull();
  await vi.advanceTimersByTimeAsync(20000);
  expect(player.snapshot().current?.id).toBe("first");
  expect(resolves("first")).toHaveLength(1);
  media.plays = null;
  void player.setPlaying(true);
  await vi.advanceTimersByTimeAsync(0);
  expect(resolves("first")).toHaveLength(2);
  expect(media.src).toContain("/first-audio/audio");
  expect(player.snapshot().playing).toBe(true);
  await vi.advanceTimersByTimeAsync(14999);
  expect(player.snapshot().current?.id).toBe("first");
  await vi.advanceTimersByTimeAsync(1);
  expect(player.snapshot().current?.id).toBe("second");
});

test.each(["pause", "select"] as const)(
  "a format result that arrives after %s cannot label or skip the chosen track",
  async (action) => {
    const media = new Media();
    let deliver!: (response: Response) => void;
    vi.stubGlobal(
      "fetch",
      vi.fn(
        () =>
          new Promise<Response>((resolve) => {
            deliver = resolve;
          }),
      ),
    );
    const resolves: string[] = [];
    const player = new Playback(media, async (recording, options) => {
      if (options.purpose === "cached")
        return recording === "first" && resolves.length === 0
          ? [{ id: "first-audio", ready: true }]
          : recording === "second"
            ? [{ id: "second-audio", ready: true }]
            : [];
      if (options.purpose !== "resolve") return [];
      resolves.push(recording);
      return resolution(
        recording,
        options.body.request_id,
        "no_suitable_source",
      );
    });
    try {
      void player.playQueue([track("first"), track("second")]);
      await vi.advanceTimersByTimeAsync(0);
      expect(media.src).toContain("/first-audio/audio");
      media.error = { code: 4 };
      media.dispatchEvent(new Event("error"));
      await vi.advanceTimersByTimeAsync(0);
      expect(resolves).toEqual(["first"]);
      if (action === "pause") await player.toggle();
      else {
        const page = await player.queuePage(0, 10);
        void player.selectEntry(page.entries[1].id);
        await vi.advanceTimersByTimeAsync(0);
        expect(media.src).toContain("/second-audio/audio");
        media.error = null;
        media.advance(1);
        expect(player.snapshot().desiredPlaying).toBeUndefined();
      }
      const before = player.snapshot();
      deliver(
        new Response("x", {
          status: 206,
          headers: { "content-type": "audio/flac" },
        }),
      );
      await vi.advanceTimersByTimeAsync(20000);
      expect(player.snapshot().error).toBe("");
      expect(player.snapshot().current?.id).toBe(before.current?.id);
      expect(player.snapshot().index).toBe(before.index);
      expect(player.snapshot().retrying ?? null).toBeNull();
      expect(resolves).toEqual(["first"]);
      if (action === "pause") {
        expect(player.snapshot().current?.id).toBe("first");
        expect(media.src).toBe("");
      } else expect(media.src).toContain("/second-audio/audio");
    } finally {
      vi.unstubAllGlobals();
    }
  },
);

test("a refused refresh stays terminal even when the cache and discovery come back empty", async () => {
  const media = new Media();
  const calls: string[] = [];
  const player = new Playback(media, async (recording, options) => {
    calls.push(`${recording}:${options.purpose}`);
    if (recording !== "first")
      return [{ id: `${recording}-audio`, ready: true }];
    if (options.purpose === "refresh") throw new HttpError(403, "Refused");
    if (options.purpose !== "resolve") return [];
    return resolution(recording, options.body.request_id, "no_suitable_source");
  });
  void player.playQueue([track("first"), track("second")]);
  await vi.advanceTimersByTimeAsync(0);
  expect(player.snapshot().current?.id).toBe("second");
  expect(player.snapshot().retrying ?? null).toBeNull();
  await vi.advanceTimersByTimeAsync(20000);
  expect(calls.filter((call) => call.startsWith("first:"))).toEqual([
    "first:cached",
    "first:refresh",
    "first:cached",
    "first:resolve",
  ]);
});

const waitingSource = (recording: string) => ({
  id: `${recording}-waiting`,
  recording_id: recording,
  provider: "plugin",
  device_id: null,
  ready: false,
  can_prepare: true,
  label: "Owned audio service",
  match_kind: "verified" as const,
});

test("a waiting source beside an unavailable service still gets three passes", async () => {
  const media = new Media();
  const calls: string[] = [];
  const player = new Playback(media, async (recording, options) => {
    calls.push(`${recording}:${options.purpose}`);
    if (recording !== "first")
      return [{ id: `${recording}-audio`, ready: true }];
    if (options.purpose !== "resolve") return [waitingSource(recording)];
    return {
      ...resolution(recording, options.body.request_id, "provider_unavailable"),
      sources: [waitingSource(recording)],
    };
  });
  void player.playQueue([track("first"), track("second")]);
  await vi.advanceTimersByTimeAsync(0);
  expect(player.snapshot().retrying).toEqual({ attempt: 2, of: 3 });
  await vi.advanceTimersByTimeAsync(500);
  expect(player.snapshot().retrying).toEqual({ attempt: 3, of: 3 });
  await vi.advanceTimersByTimeAsync(1000);
  expect(player.snapshot().current?.id).toBe("second");
  const pass = ["first:cached", "first:refresh", "first:resolve"];
  expect(calls.filter((call) => call.startsWith("first:"))).toEqual([
    ...pass,
    ...pass,
    ...pass,
  ]);
});

test("a waiting cached source after an exhausted lookup transport still gets three passes", async () => {
  const media = new Media();
  const requests: string[] = [];
  const player = new Playback(media, async (recording, options) => {
    if (recording !== "first")
      return [{ id: `${recording}-audio`, ready: true }];
    if (options.purpose !== "resolve") return [waitingSource(recording)];
    requests.push(options.body.request_id);
    throw new TypeError("Owned network failure");
  });
  void player.playQueue([track("first"), track("second")]);
  await vi.advanceTimersByTimeAsync(0);
  expect(player.snapshot().retrying).toEqual({ attempt: 2, of: 3 });
  await vi.advanceTimersByTimeAsync(500);
  expect(player.snapshot().retrying).toEqual({ attempt: 3, of: 3 });
  await vi.advanceTimersByTimeAsync(1000);
  expect(player.snapshot().current?.id).toBe("second");
  expect(requests).toHaveLength(6);
  expect(requests[1]).toBe(requests[0]);
  expect(requests[3]).toBe(requests[2]);
  expect(requests[5]).toBe(requests[4]);
  expect(new Set(requests).size).toBe(3);
});

test("a progress save that blocks an unheard track settles the search without skipping, and Play starts a fresh bounded search", async () => {
  const media = new Media();
  let admit = false;
  const admitted: number[] = [];
  const { resolver, resolves } = discovery(() => "ready");
  const player = new Playback(media, resolver, (_track, position) => {
    if (!admit) return false;
    admitted.push(position);
    return true;
  });
  void player.playQueue([track("first"), track("second")]);
  await vi.advanceTimersByTimeAsync(0);
  expect(media.src).toContain("/first-audio/audio");
  media.dispatchEvent(new Event("loadedmetadata"));
  expect(player.snapshot().desiredPlaying).toBe(true);
  player.seek(20);
  media.dispatchEvent(new Event("seeked"));
  media.advance(20);
  expect(media.paused).toBe(true);
  expect(player.snapshot().error).toBe(
    "Listening progress is waiting to be saved. Playback paused.",
  );
  expect(player.snapshot().desiredPlaying).toBeUndefined();
  expect(player.snapshot().playing).toBe(false);
  expect(player.snapshot().retrying ?? null).toBeNull();
  const source = media.src;
  await vi.advanceTimersByTimeAsync(30_000);
  expect(player.snapshot().current?.id).toBe("first");
  expect(media.src).toBe(source);
  expect(resolves("first")).toHaveLength(1);
  expect(resolves("second")).toHaveLength(0);
  admit = true;
  await player.setPlaying(true);
  expect(player.snapshot().desiredPlaying).toBe(true);
  expect(media.paused).toBe(false);
  expect(media.src).toBe(source);
  media.advance(20);
  expect(player.snapshot().desiredPlaying).toBe(true);
  media.advance(20.5);
  expect(player.snapshot().desiredPlaying).toBeUndefined();
  expect(admitted).toContain(20);
  await vi.advanceTimersByTimeAsync(30_000);
  expect(player.snapshot().current?.id).toBe("first");
  expect(resolves("first")).toHaveLength(1);
});

test("a progress block on an unheard track that a later Play never resumes still cannot skip it", async () => {
  const media = new Media();
  const { resolver, resolves } = discovery(() => "ready");
  const player = new Playback(media, resolver, () => false);
  void player.playQueue([track("first"), track("second")]);
  await vi.advanceTimersByTimeAsync(0);
  media.dispatchEvent(new Event("loadedmetadata"));
  player.seek(20);
  media.dispatchEvent(new Event("seeked"));
  media.advance(20);
  expect(player.snapshot().playing).toBe(false);
  media.dispatchEvent(new Event("play"));
  expect(media.paused).toBe(true);
  expect(player.snapshot().playing).toBe(false);
  await vi.advanceTimersByTimeAsync(30_000);
  expect(player.snapshot().current?.id).toBe("first");
  expect(resolves("first")).toHaveLength(1);
});

test("a resumed unheard track that never advances still expires at its fresh fifteen seconds and advances once", async () => {
  const media = new Media();
  let admit = false;
  const { resolver, resolves } = discovery(() => "ready");
  const player = new Playback(media, resolver, () => admit);
  void player.playQueue([track("first"), track("second")]);
  await vi.advanceTimersByTimeAsync(0);
  media.dispatchEvent(new Event("loadedmetadata"));
  player.seek(20);
  media.dispatchEvent(new Event("seeked"));
  media.advance(20);
  expect(player.snapshot().playing).toBe(false);
  await vi.advanceTimersByTimeAsync(30_000);
  admit = true;
  await player.setPlaying(true);
  expect(player.snapshot().desiredPlaying).toBe(true);
  await vi.advanceTimersByTimeAsync(14_999);
  expect(player.snapshot().current?.id).toBe("first");
  await vi.advanceTimersByTimeAsync(1);
  expect(player.snapshot().current?.id).toBe("second");
  expect(player.snapshot().index).toBe(1);
  await vi.advanceTimersByTimeAsync(30_000);
  expect(resolves("first")).toHaveLength(1);
});

test.each(["resolves", "rejects"] as const)(
  "a play still pending when a progress save blocks the unheard track %s late without restarting or skipping it",
  async (outcome) => {
    const media = new Media();
    let finish!: () => void;
    let fail!: (error: Error) => void;
    media.plays = () =>
      new Promise<void>((resolve, reject) => {
        finish = resolve;
        fail = reject;
      });
    const { resolver, resolves } = discovery(() => "ready");
    const player = new Playback(media, resolver, () => false);
    void player.playQueue([track("first"), track("second")]);
    await vi.advanceTimersByTimeAsync(0);
    expect(media.paused).toBe(true);
    media.dispatchEvent(new Event("loadedmetadata"));
    player.seek(20);
    media.dispatchEvent(new Event("seeked"));
    media.advance(20);
    expect(player.snapshot().error).toBe(
      "Listening progress is waiting to be saved. Playback paused.",
    );
    expect(player.snapshot().desiredPlaying).toBeUndefined();
    media.plays = null;
    if (outcome === "resolves") finish();
    else fail(Object.assign(new Error("Aborted"), { name: "AbortError" }));
    await vi.advanceTimersByTimeAsync(0);
    expect(media.paused).toBe(true);
    expect(player.snapshot().playing).toBe(false);
    expect(player.snapshot().error).toBe(
      "Listening progress is waiting to be saved. Playback paused.",
    );
    await vi.advanceTimersByTimeAsync(30_000);
    expect(player.snapshot().current?.id).toBe("first");
    expect(resolves("first")).toHaveLength(1);
    expect(resolves("second")).toHaveLength(0);
  },
);

function overdueClock() {
  let now = 1_000;
  vi.spyOn(performance, "now").mockImplementation(() => now);
  return {
    pastDeadline: () => {
      now += 15_001;
    },
  };
}

const events = () => logged.map((values) => values[0]);

test("progress that arrives after fifteen seconds while the expiry timer is still queued ends the search instead of counting as heard", async () => {
  const clock = overdueClock();
  const media = new Media();
  const { resolver, resolves } = discovery(() => "ready");
  const player = new Playback(media, resolver);
  void player.playQueue([track("first"), track("second")]);
  await vi.advanceTimersByTimeAsync(0);
  expect(media.src).toContain("/first-audio/audio");
  expect(player.snapshot().desiredPlaying).toBe(true);
  clock.pastDeadline();
  media.advance(1);
  await vi.advanceTimersByTimeAsync(0);
  expect(events()).not.toContain("playback.acquisition_heard");
  expect(events()).toContain("playback.acquisition_expired");
  expect(player.snapshot().current?.id).toBe("second");
  expect(player.snapshot().index).toBe(1);
  media.advance(2);
  expect(player.snapshot().desiredPlaying).toBeUndefined();
  await vi.advanceTimersByTimeAsync(20_000);
  expect(
    events().filter((event) => event === "playback.acquisition_expired"),
  ).toHaveLength(1);
  expect(resolves("first")).toHaveLength(1);
});

test("a lookup that answers after fifteen seconds while the expiry timer is still queued advances once without another pass", async () => {
  const clock = overdueClock();
  const media = new Media();
  let finish!: (value: Schema<"SourceResolution">) => void;
  let held = "";
  const { resolver, resolves } = discovery((recording, _pass, request) => {
    if (recording !== "first") return "ready";
    held = request;
    return new Promise((resolve) => {
      finish = resolve;
    });
  });
  const player = new Playback(media, resolver);
  void player.playQueue([track("first"), track("second")]);
  await vi.advanceTimersByTimeAsync(0);
  expect(resolves("first")).toHaveLength(1);
  clock.pastDeadline();
  finish(resolution("first", held, "provider_unavailable"));
  await vi.advanceTimersByTimeAsync(0);
  expect(events()).not.toContain("playback.acquisition_retry");
  expect(player.snapshot().retrying ?? null).toBeNull();
  expect(player.snapshot().current?.id).toBe("second");
  media.advance(1);
  expect(player.snapshot().desiredPlaying).toBeUndefined();
  await vi.advanceTimersByTimeAsync(20_000);
  expect(
    events().filter((event) => event === "playback.acquisition_expired"),
  ).toHaveLength(1);
  expect(resolves("first")).toHaveLength(1);
});

test("a ready lookup that answers after fifteen seconds while the expiry timer is still queued never opens its audio", async () => {
  const clock = overdueClock();
  const media = new Media();
  let finish!: (value: Schema<"SourceResolution">) => void;
  let held = "";
  const opened: string[] = [];
  const { resolver, resolves } = discovery((recording, _pass, request) => {
    if (recording !== "first") return "ready";
    held = request;
    return new Promise((resolve) => {
      finish = resolve;
    });
  });
  const sourceOpens = vi.spyOn(BrowserStream.prototype, "openSource");
  const player = new Playback(media, resolver);
  media.addEventListener("play", () => opened.push(media.src));
  void player.playQueue([track("first"), track("second")]);
  await vi.advanceTimersByTimeAsync(0);
  expect(resolves("first")).toHaveLength(1);
  clock.pastDeadline();
  finish(resolution("first", held, "ready", "late-first-audio"));
  await vi.advanceTimersByTimeAsync(0);
  expect(opened.some((src) => src.includes("/late-first-audio/"))).toBe(false);
  expect(events()).not.toContain("playback.acquisition_heard");
  expect(player.snapshot().current?.id).toBe("second");
  expect(media.src).toContain("/second-audio/audio");
  media.advance(1);
  expect(player.snapshot().desiredPlaying).toBeUndefined();
  await vi.advanceTimersByTimeAsync(20_000);
  expect(
    events().filter((event) => event === "playback.acquisition_expired"),
  ).toHaveLength(1);
  expect(resolves("first")).toHaveLength(1);
  expect(opened.some((src) => src.includes("/late-first-audio/"))).toBe(false);
  expect(sourceOpens.mock.calls.map(([source]) => source)).not.toContain(
    "late-first-audio",
  );
  expect(sourceOpens.mock.calls.map(([source]) => source)).toContain(
    "second-audio",
  );
});

test("a prepared next track that becomes ready after fifteen seconds while the expiry timer is still queued is never adopted", async () => {
  const clock = overdueClock();
  let release!: (sources: Source[]) => void;
  const held = new Promise<Source[]>((resolve) => {
    release = resolve;
  });
  const harness = browserPreparationModes(async (id, options) =>
    id === "second" && options.purpose === "cached"
      ? held
      : [{ id: `${id}-audio`, ready: true }],
  );
  let failure: unknown = null;
  try {
    void harness.player.playQueue([recording("first"), recording("second")]);
    await vi.advanceTimersByTimeAsync(0);
    expect(harness.initial.played).toEqual([audioPath("first-audio")]);
    harness.initial.complete();
    await vi.advanceTimersByTimeAsync(0);
    expect(harness.player.snapshot().current?.id).toBe("second");
    clock.pastDeadline();
    release([{ id: "second-audio", ready: true }]);
    const buffered = await harness.buffer("second-audio");
    buffered.ready();
    await vi.advanceTimersByTimeAsync(0);
    expect(harness.initial.loads).not.toContain(audioPath("second-audio"));
    expect(harness.initial.played).not.toContain(audioPath("second-audio"));
    expect(buffered.played).toEqual([]);
    expect(events()).not.toContain("playback.acquisition_heard");
    expect(
      events().filter((event) => event === "playback.acquisition_expired"),
    ).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(20_000);
    expect(
      events().filter((event) => event === "playback.acquisition_expired"),
    ).toHaveLength(1);
    expect(harness.initial.played).toEqual([audioPath("first-audio")]);
  } catch (error) {
    failure = error;
  } finally {
    try {
      await harness.close();
    } catch (error) {
      failure ??= error;
    } finally {
      vi.unstubAllGlobals();
    }
  }
  if (failure !== null) throw failure;
});
