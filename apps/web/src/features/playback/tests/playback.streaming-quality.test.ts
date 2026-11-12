import { expect, test, vi } from "vitest";
import { Playback, type MediaPort } from "../playback";
import type { SourceResolver } from "../api/playbackSources";
import { setStreamingQuality } from "../browser/StreamingQuality";

import {
  StreamMedia as Media,
  streamTrack as track,
  convertedHead,
} from "../fixtures/browserStreamMedia";

test("selected browser quality resumes and seeks a converted stream on the full recording timeline", async () => {
  localStorage.setItem("hito:stream-quality", "high");
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, options?: RequestInit) => {
      expect(options?.method).toBe("HEAD");
      return new Response(null, {
        headers: {
          "X-Hito-Stream-Quality": "high",
          "X-Hito-Stream-Start-Ms":
            new URL(String(input), "http://hito.invalid").searchParams.get(
              "start_ms",
            ) ?? "0",
        },
      });
    }),
  );
  const media = new Media();
  const player = new Playback(media, async () => [
    { id: "source", ready: true },
  ]);
  await player.playQueue([track], 0, { kind: "position", position: 12 });
  const requested = new URL(media.src, "http://hito.invalid");
  expect(requested.searchParams.get("quality")).toBe("high");
  expect(requested.searchParams.get("start_ms")).toBe("12000");
  media.ready(78);
  expect(media.currentTime).toBe(0);
  expect(player.positionSnapshot()).toEqual({ position: 12, duration: 90 });
  media.advance(2);
  expect(player.positionSnapshot()).toEqual({ position: 14, duration: 90 });
  player.seek(40);
  await vi.waitFor(() => {
    expect(
      new URL(media.src, "http://hito.invalid").searchParams.get("start_ms"),
    ).toBe("40000");
  });
  media.ready(50);
  media.advance(1);
  expect(player.positionSnapshot()).toEqual({ position: 41, duration: 90 });
  player.retire();
});

test("a failed converted stream tries the same source Original once before ordinary source recovery", async () => {
  localStorage.setItem("hito:stream-quality", "high");
  vi.stubGlobal(
    "fetch",
    vi.fn(
      async (input: RequestInfo | URL) =>
        new Response(null, {
          headers: {
            "X-Hito-Stream-Quality": "high",
            "X-Hito-Stream-Start-Ms":
              new URL(String(input), "http://hito.invalid").searchParams.get(
                "start_ms",
              ) ?? "0",
          },
        }),
    ),
  );
  const media = new Media();
  const resolve = vi.fn<SourceResolver>(async (_id, options) => [
    {
      id: options.purpose === "refresh" ? "replacement" : "source",
      ready: true,
    },
  ]);
  const player = new Playback(media, resolve);
  await player.playQueue([track], 0, { kind: "position", position: 12 });
  media.ready(78);
  media.advance(2);
  media.error = { code: 2 };
  media.dispatchEvent(new Event("error"));
  await vi.waitFor(() =>
    expect(media.src).toBe("/api/v1/sources/source/audio"),
  );
  media.ready(90);
  expect(media.currentTime).toBe(14);
  expect(player.positionSnapshot()).toEqual({ position: 14, duration: 90 });
  expect(player.snapshot().playing).toBe(true);
  expect(resolve).toHaveBeenCalledTimes(1);
  media.error = { code: 2 };
  media.dispatchEvent(new Event("error"));
  await vi.waitFor(() => expect(media.src).toContain("/replacement/audio"));
  expect(resolve).toHaveBeenCalledTimes(2);
  player.retire();
});

test("a listener can pause a converted seek while quality negotiation is pending", async () => {
  localStorage.setItem("hito:stream-quality", "high");
  let release!: () => void;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  let calls = 0;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL) => {
      if (++calls === 2) await held;
      return convertedHead(input);
    }),
  );
  const media = new Media();
  const player = new Playback(media, async () => [
    { id: "source", ready: true },
  ]);
  await player.playQueue([track]);
  media.ready();
  media.advance(3);
  player.seek(40);
  await player.setPlaying(false);
  expect(player.snapshot().playing).toBe(false);
  release();
  await vi.waitFor(() => expect(media.src).toContain("start_ms=40000"));
  media.ready(50);
  expect(media.paused).toBe(true);
  expect(player.positionSnapshot()).toEqual({ position: 40, duration: 90 });
  await player.setPlaying(true);
  media.advance(1);
  expect(player.positionSnapshot().position).toBe(41);
  player.retire();
});

test("the latest seek to zero stays visible while an earlier converted seek finishes loading", async () => {
  localStorage.setItem("hito:stream-quality", "saver");
  let release!: () => void;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  let calls = 0;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL) => {
      if (++calls === 2) await held;
      return convertedHead(input);
    }),
  );
  const media = new Media();
  const player = new Playback(media, async () => [
    { id: "source", ready: true },
  ]);
  await player.playQueue([track], 0, { kind: "position", position: 12 });
  media.ready(78);
  player.seek(40);
  player.seek(0);
  release();
  await vi.waitFor(() => expect(media.src).toContain("start_ms=40000"));
  media.advance(0.1);
  expect(player.positionSnapshot().position).toBe(0);
  media.ready(50);
  await vi.waitFor(() => expect(media.src).toContain("start_ms=0"));
  media.ready(90);
  media.advance(1);
  expect(player.positionSnapshot()).toEqual({ position: 1, duration: 90 });
  player.retire();
});

test("changing browser quality replaces the next-entry preload and adopts that choice without interrupting current audio", async () => {
  setStreamingQuality("high");
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL) => convertedHead(input)),
  );
  const media = new Media();
  const prepared: Media[] = [];
  const released: MediaPort[] = [];
  const resolve = vi.fn<SourceResolver>(async (recording) => [
    { id: `${recording}-source`, ready: true },
  ]);
  const player = new Playback(media, resolve, undefined, {
    create: () => {
      const next = new Media();
      prepared.push(next);
      return next;
    },
    release: (media) => {
      released.push(media);
    },
  });
  const next = { ...track, id: "next" };
  await player.playQueue([track, next]);
  media.ready();
  await vi.waitFor(() =>
    expect(prepared[0]?.src).toContain("/next-source/audio"),
  );
  expect(
    new URL(prepared[0].src, "http://hito.invalid").searchParams.get("quality"),
  ).toBe("high");
  const currentUrl = media.src;
  setStreamingQuality("saver");
  await vi.waitFor(() => expect(prepared[1]?.src).toContain("quality=saver"));
  expect(released).toContain(prepared[0]);
  expect(prepared[0].src).toBe("");
  expect(media.src).toBe(currentUrl);
  expect(media.paused).toBe(false);
  prepared[1].ready();
  media.ended = true;
  media.dispatchEvent(new Event("ended"));
  await vi.waitFor(() =>
    expect(media.src).toContain("/next-source/audio?quality=saver"),
  );
  expect(player.snapshot().current?.id).toBe("next");
  expect(
    resolve.mock.calls.every(([, options]) => options.purpose === "cached"),
  ).toBe(true);
  player.retire();
});

test("a converted recording without catalog duration retains the full duration learned before seeking", async () => {
  setStreamingQuality("high");
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL) => convertedHead(input)),
  );
  const media = new Media();
  const player = new Playback(media, async () => [
    { id: "source", ready: true },
  ]);
  await player.playQueue([{ ...track, duration_ms: 0 }]);
  media.ready(90);
  expect(player.positionSnapshot().duration).toBe(90);
  player.seek(40);
  await vi.waitFor(() => expect(media.src).toContain("start_ms=40000"));
  media.ready(50);
  media.advance(1);
  expect(player.positionSnapshot()).toEqual({ position: 41, duration: 90 });
  player.retire();
});

test("an unknown-duration converted resume keeps the full duration unknown instead of publishing its suffix", async () => {
  setStreamingQuality("saver");
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL) => convertedHead(input)),
  );
  const media = new Media();
  const player = new Playback(media, async () => [
    { id: "source", ready: true },
  ]);
  await player.playQueue([{ ...track, duration_ms: 0 }], 0, {
    kind: "position",
    position: 12,
  });
  media.ready(78);
  media.advance(1);
  expect(player.positionSnapshot()).toEqual({ position: 13, duration: 0 });
  player.retire();
});
