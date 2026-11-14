import { afterEach, expect, test, vi } from "vitest";
import { onInvalidated, type InvalidationScope } from "../invalidation";
import {
  changeStreamPath,
  noticeInvalidations,
  openChangeStream,
  sessionStillActive,
} from "../changeStream";
import { getConnection } from "../connection";

class FakeSource {
  static all: FakeSource[] = [];
  onmessage: ((event: { data: string }) => void) | null = null;
  onerror: (() => void) | null = null;
  closed = false;
  constructor(
    readonly url: string,
    readonly init?: EventSourceInit,
  ) {
    FakeSource.all.push(this);
  }
  close() {
    this.closed = true;
  }
  send(notice: unknown) {
    this.onmessage?.({ data: JSON.stringify(notice) });
  }
  fail() {
    this.onerror?.();
  }
}

type Seen = { scope: InvalidationScope; id: string | null };

function watch() {
  const seen: Seen[] = [];
  const scopes: InvalidationScope[] = [
    "sources",
    "resolved",
    "recording",
    "playlist",
    "playlists",
    "artwork",
  ];
  const stops = scopes.map((scope) =>
    onInvalidated(scope, (id) => seen.push({ scope, id })),
  );
  return { seen, stop: () => stops.forEach((stop) => stop()) };
}

function open(sessionActive: () => Promise<boolean> = async () => true) {
  FakeSource.all = [];
  return openChangeStream({
    eventSource: FakeSource as unknown as typeof EventSource,
    sessionActive,
    delays: [10, 20],
  });
}

afterEach(() => {
  FakeSource.all = [];
});

test("opens one credentialed stream against the change events route", () => {
  const stop = open();
  expect(FakeSource.all).toHaveLength(1);
  expect(FakeSource.all[0].url).toBe(changeStreamPath);
  expect(changeStreamPath).toBe("/api/v1/changes/events");
  expect(FakeSource.all[0].init?.withCredentials).toBe(true);
  stop();
  expect(FakeSource.all[0].closed).toBe(true);
});

test("maps each notice kind to its invalidation scopes", () => {
  expect(
    noticeInvalidations({ kind: "sources", recording_id: "r" }, false),
  ).toEqual([
    { scope: "sources", id: "r" },
    { scope: "resolved", id: "r" },
  ]);
  expect(
    noticeInvalidations({ kind: "recording", recording_id: "r" }, false),
  ).toEqual([{ scope: "recording", id: "r" }]);
  expect(noticeInvalidations({ kind: "recordings" }, false)).toEqual([
    { scope: "recording", id: null },
  ]);
  expect(
    noticeInvalidations({ kind: "playlist", playlist_id: "p" }, false),
  ).toEqual([{ scope: "playlist", id: "p" }]);
  expect(noticeInvalidations({ kind: "playlists" }, false)).toEqual([
    { scope: "playlists", id: null },
  ]);
  expect(
    noticeInvalidations(
      { kind: "artwork", entity: "playlist", id: "p" },
      false,
    ),
  ).toEqual([{ scope: "artwork", id: "playlist:p" }]);
  expect(noticeInvalidations({ kind: "novel" } as never, false)).toEqual([]);
});

test("the first connected resync of a page load is ignored but later ones are not", () => {
  const { seen, stop: unwatch } = watch();
  const stop = open();
  const source = FakeSource.all[0];
  source.send({ kind: "resync", reason: "connected" });
  expect(seen).toEqual([]);
  source.send({ kind: "resync", reason: "lagged" });
  expect(seen.map((entry) => entry.scope).sort()).toEqual([
    "playlist",
    "playlists",
    "recording",
    "resolved",
    "sources",
  ]);
  seen.length = 0;
  source.send({ kind: "resync", reason: "connected" });
  expect(seen.length).toBeGreaterThan(0);
  stop();
  unwatch();
});

test("delivers notices to invalidation listeners and survives bad frames", () => {
  const { seen, stop: unwatch } = watch();
  const stop = open();
  const source = FakeSource.all[0];
  source.onmessage?.({ data: "not json" });
  source.onmessage?.({ data: "null" });
  source.send({ kind: "playlist", playlist_id: "p1" });
  expect(seen).toEqual([{ scope: "playlist", id: "p1" }]);
  stop();
  unwatch();
});

test("reconnects with growing backoff while the session is active", async () => {
  vi.useFakeTimers();
  const stop = open();
  FakeSource.all[0].fail();
  expect(FakeSource.all[0].closed).toBe(true);
  await vi.advanceTimersByTimeAsync(9);
  expect(FakeSource.all).toHaveLength(1);
  await vi.advanceTimersByTimeAsync(1);
  expect(FakeSource.all).toHaveLength(2);
  FakeSource.all[1].fail();
  await vi.advanceTimersByTimeAsync(19);
  expect(FakeSource.all).toHaveLength(2);
  await vi.advanceTimersByTimeAsync(1);
  expect(FakeSource.all).toHaveLength(3);
  stop();
  FakeSource.all[2].fail();
  await vi.advanceTimersByTimeAsync(1000);
  expect(FakeSource.all).toHaveLength(3);
});

test("a connected notice resets the backoff", async () => {
  vi.useFakeTimers();
  const stop = open();
  FakeSource.all[0].fail();
  await vi.advanceTimersByTimeAsync(10);
  FakeSource.all[1].fail();
  await vi.advanceTimersByTimeAsync(20);
  FakeSource.all[2].send({ kind: "resync", reason: "connected" });
  FakeSource.all[2].fail();
  await vi.advanceTimersByTimeAsync(10);
  expect(FakeSource.all).toHaveLength(4);
  stop();
});

test("does not reconnect when the session is gone or locked", async () => {
  vi.useFakeTimers();
  const stop = open(async () => false);
  FakeSource.all[0].fail();
  await vi.advanceTimersByTimeAsync(1000);
  expect(FakeSource.all).toHaveLength(1);
  stop();
});

test("a failed session check keeps retrying", async () => {
  vi.useFakeTimers();
  let checks = 0;
  const stop = open(async () => {
    checks++;
    if (checks === 1) throw new Error("offline");
    return true;
  });
  FakeSource.all[0].fail();
  await vi.advanceTimersByTimeAsync(10);
  expect(FakeSource.all).toHaveLength(2);
  stop();
});

test("closing during the session check opens nothing", async () => {
  vi.useFakeTimers();
  const stop = open(async () => true);
  FakeSource.all[0].fail();
  stop();
  await vi.advanceTimersByTimeAsync(1000);
  expect(FakeSource.all).toHaveLength(1);
});

test("is inert when the browser has no EventSource", () => {
  const stop = openChangeStream({ eventSource: undefined });
  expect(() => stop()).not.toThrow();
});

test("the session check reads the account without touching the connection banner", async () => {
  const controller = new AbortController();
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => new Response("{}", { status: 200 })),
  );
  expect(await sessionStillActive(controller.signal)).toBe(true);
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => new Response("", { status: 401 })),
  );
  expect(await sessionStillActive(controller.signal)).toBe(false);
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => new Response("", { status: 403 })),
  );
  expect(await sessionStillActive(controller.signal)).toBe(false);
  const locked = vi.fn();
  window.addEventListener("hito-session-locked", locked);
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => new Response("", { status: 423 })),
  );
  expect(await sessionStillActive(controller.signal)).toBe(false);
  expect(locked).toHaveBeenCalledTimes(1);
  window.removeEventListener("hito-session-locked", locked);
  expect(getConnection().reachable).toBe(true);
});

test("a failing, gateway-erroring or unanswering session check never raises the outage banner", async () => {
  const controller = new AbortController();
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => Promise.reject(new TypeError("down"))),
  );
  await expect(sessionStillActive(controller.signal)).rejects.toThrow();
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => new Response("", { status: 502 })),
  );
  await expect(sessionStillActive(controller.signal)).rejects.toThrow();
  vi.useFakeTimers();
  vi.stubGlobal(
    "fetch",
    vi.fn(
      (_url: string, init: RequestInit) =>
        new Promise((_resolve, reject) =>
          init.signal?.addEventListener("abort", () =>
            reject(init.signal?.reason),
          ),
        ),
    ),
  );
  const pending = expect(
    sessionStillActive(controller.signal),
  ).rejects.toBeDefined();
  await vi.advanceTimersByTimeAsync(10_000);
  await pending;
  expect(getConnection().reachable).toBe(true);
});
