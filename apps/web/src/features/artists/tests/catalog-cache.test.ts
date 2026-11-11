import { act, renderHook, waitFor } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { ConnectionError } from "../../../shared/connection";
import { HttpError, lockedEvent } from "../../../shared/http";
import { invalidate } from "../../../shared/invalidation";
import {
  FRESH_MS,
  STALE_MS,
  cachedRead,
  catalogCacheClock,
  forgetCatalog,
  rememberReleaseGroups,
  rememberedReleaseGroup,
  useCatalogRefreshing,
} from "../api/catalogCache";

const signal = new AbortController().signal;
const available = (value: string) => ({ status: "available", value });

function clock() {
  let now = 1_000;
  catalogCacheClock.set(() => now);
  return (ms: number) => {
    now += ms;
  };
}

test("a fresh answer is served without asking the provider again", async () => {
  clock();
  const load = vi.fn(async () => available("one"));
  await cachedRead("a", ["x"], load, signal);
  const again = await cachedRead("a", ["x"], load, signal);
  expect(again.value).toBe("one");
  expect(load).toHaveBeenCalledTimes(1);
});

test("accounts and request bodies never share an answer", async () => {
  clock();
  const load = vi.fn(async () => available("one"));
  await cachedRead("a", ["x"], load, signal);
  await cachedRead("b", ["x"], load, signal);
  await cachedRead("a", ["y"], load, signal);
  expect(load).toHaveBeenCalledTimes(3);
});

test("unavailable outcomes are not remembered", async () => {
  clock();
  const load = vi
    .fn<() => Promise<{ status: string; value?: string }>>()
    .mockResolvedValueOnce({ status: "unavailable" })
    .mockResolvedValueOnce(available("two"));
  expect((await cachedRead("a", ["x"], load, signal)).status).toBe(
    "unavailable",
  );
  expect((await cachedRead("a", ["x"], load, signal)).value).toBe("two");
});

test("a stale answer shows at once and refreshes quietly in the background", async () => {
  const advance = clock();
  const gate: { release: (value: unknown) => void } = {
    release: () => undefined,
  };
  const load = vi
    .fn<() => Promise<{ status: string; value: string }>>()
    .mockResolvedValueOnce(available("old"))
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          gate.release = resolve as (value: unknown) => void;
        }),
    );
  await cachedRead("a", ["x"], load, signal);
  advance(FRESH_MS + 1);
  const view = renderHook(() => useCatalogRefreshing());
  expect(view.result.current).toBe(false);
  const shown = await cachedRead("a", ["x"], load, signal);
  expect(shown.value).toBe("old");
  await waitFor(() => expect(view.result.current).toBe(true));
  await act(async () => gate.release(available("new")));
  await waitFor(() => expect(view.result.current).toBe(false));
  expect((await cachedRead("a", ["x"], load, signal)).value).toBe("new");
  expect(load).toHaveBeenCalledTimes(2);
});

test("an answer older than the stale limit blocks on a fresh read", async () => {
  const advance = clock();
  const load = vi
    .fn<() => Promise<{ status: string; value: string }>>()
    .mockResolvedValueOnce(available("old"))
    .mockResolvedValueOnce(available("new"));
  await cachedRead("a", ["x"], load, signal);
  advance(STALE_MS + 1);
  expect((await cachedRead("a", ["x"], load, signal)).value).toBe("new");
});

test("forgetting one account keeps the others", async () => {
  clock();
  const load = vi.fn(async () => available("one"));
  await cachedRead("a", ["x"], load, signal);
  await cachedRead("b", ["x"], load, signal);
  rememberReleaseGroups("a", "assoc", [
    { id: "g", title: "T", secondary_types: [] },
  ]);
  forgetCatalog("a");
  expect(rememberedReleaseGroup("a", "assoc", "g")).toBeNull();
  await cachedRead("b", ["x"], load, signal);
  expect(load).toHaveBeenCalledTimes(2);
});

test("a revisit whose background refresh fails keeps showing the first result", async () => {
  const advance = clock();
  catalogCacheClock.setRetryDelay(0);
  const load = vi
    .fn<() => Promise<{ status: string; value?: string }>>()
    .mockResolvedValueOnce(available("first"))
    .mockResolvedValue({ status: "unavailable" });
  await cachedRead("a", ["x"], load, signal);
  advance(FRESH_MS + 1);
  const view = renderHook(() => useCatalogRefreshing());
  expect((await cachedRead("a", ["x"], load, signal)).value).toBe("first");
  await waitFor(() => expect(load).toHaveBeenCalledTimes(2));
  await waitFor(() => expect(view.result.current).toBe(false));
  advance(1);
  expect((await cachedRead("a", ["x"], load, signal)).value).toBe("first");
});

test("an expired answer is still shown when the blocking read fails", async () => {
  const advance = clock();
  catalogCacheClock.setRetryDelay(0);
  const load = vi
    .fn<() => Promise<{ status: string; value?: string }>>()
    .mockResolvedValueOnce(available("first"))
    .mockRejectedValue(new HttpError(500, "broken"));
  await cachedRead("a", ["x"], load, signal);
  advance(STALE_MS + 1);
  expect((await cachedRead("a", ["x"], load, signal)).value).toBe("first");
});

test("a transient failure is retried once and a permanent one is not", async () => {
  clock();
  catalogCacheClock.setRetryDelay(0);
  for (const error of [
    new HttpError(429, "slow down"),
    new HttpError(503, "busy"),
    new ConnectionError("timeout"),
  ]) {
    catalogCacheClock.reset();
    catalogCacheClock.setRetryDelay(0);
    const load = vi
      .fn<() => Promise<{ status: string; value?: string }>>()
      .mockRejectedValueOnce(error)
      .mockResolvedValueOnce(available("ok"));
    expect((await cachedRead("a", ["x"], load, signal)).value).toBe("ok");
    expect(load).toHaveBeenCalledTimes(2);
  }
  const permanent = vi
    .fn<() => Promise<{ status: string }>>()
    .mockRejectedValue(new HttpError(404, "gone"));
  await expect(cachedRead("a", ["y"], permanent, signal)).rejects.toThrow(
    "gone",
  );
  expect(permanent).toHaveBeenCalledTimes(1);
  const unretried = vi
    .fn<() => Promise<{ status: string }>>()
    .mockRejectedValue(new HttpError(503, "busy"));
  await expect(
    cachedRead("a", ["w"], unretried, signal, false, false),
  ).rejects.toThrow("busy");
  expect(unretried).toHaveBeenCalledTimes(1);
  const broken = vi
    .fn<() => Promise<{ status: string }>>()
    .mockRejectedValue(new HttpError(503, "busy"));
  await expect(cachedRead("a", ["z"], broken, signal)).rejects.toThrow("busy");
  expect(broken).toHaveBeenCalledTimes(2);
});

test("the retry waits for the backoff and stops when the caller cancels", async () => {
  clock();
  catalogCacheClock.setRetryDelay(700);
  vi.useFakeTimers();
  const load = vi
    .fn<(signal: AbortSignal) => Promise<{ status: string }>>()
    .mockRejectedValue(new HttpError(503, "busy"));
  const caller = new AbortController();
  const pending = cachedRead("a", ["x"], load, caller.signal);
  const outcome = expect(pending).rejects.toBeDefined();
  await vi.advanceTimersByTimeAsync(100);
  expect(load).toHaveBeenCalledTimes(1);
  caller.abort();
  await outcome;
  await vi.advanceTimersByTimeAsync(10_000);
  expect(load).toHaveBeenCalledTimes(1);
});

test("simultaneous reads of one request share one load", async () => {
  clock();
  const gate: { release: (value: unknown) => void } = {
    release: () => undefined,
  };
  const load = vi.fn(
    () =>
      new Promise<{ status: string; value: string }>((resolve) => {
        gate.release = resolve as (value: unknown) => void;
      }),
  );
  const first = cachedRead("a", ["x"], load, signal);
  const second = cachedRead("a", ["x"], load, signal);
  await Promise.resolve();
  gate.release(available("one"));
  expect((await first).value).toBe("one");
  expect((await second).value).toBe("one");
  expect(load).toHaveBeenCalledTimes(1);
});

test("one caller leaving keeps the shared load running for the other", async () => {
  clock();
  const gate: { release: (value: unknown) => void } = {
    release: () => undefined,
  };
  let shared: AbortSignal | undefined;
  const load = vi.fn((loadSignal: AbortSignal) => {
    shared = loadSignal;
    return new Promise<{ status: string; value: string }>((resolve) => {
      gate.release = resolve as (value: unknown) => void;
    });
  });
  const leaving = new AbortController();
  const gone = cachedRead("a", ["x"], load, leaving.signal);
  const staying = cachedRead("a", ["x"], load, signal);
  const left = expect(gone).rejects.toBeDefined();
  leaving.abort();
  await left;
  expect(shared?.aborted).toBe(false);
  gate.release(available("one"));
  expect((await staying).value).toBe("one");
});

test("the oldest untouched answer is evicted first and a read keeps one alive", async () => {
  clock();
  const load = vi.fn(async () => available("v"));
  for (let index = 0; index < 200; index++)
    await cachedRead("a", [index], load, signal);
  await cachedRead("a", [0], load, signal);
  await cachedRead("a", ["extra"], load, signal);
  expect(load).toHaveBeenCalledTimes(201);
  await cachedRead("a", [0], load, signal);
  expect(load).toHaveBeenCalledTimes(201);
  await cachedRead("a", [1], load, signal);
  expect(load).toHaveBeenCalledTimes(202);
});

test("locking the session clears every remembered answer", async () => {
  clock();
  const load = vi.fn(async () => available("one"));
  await cachedRead("a", ["x"], load, signal);
  window.dispatchEvent(new Event(lockedEvent));
  await cachedRead("a", ["x"], load, signal);
  expect(load).toHaveBeenCalledTimes(2);
});

test("a library linkage change shows the old answer and refreshes it", async () => {
  clock();
  const load = vi
    .fn<() => Promise<{ status: string; value: string }>>()
    .mockResolvedValueOnce(available("old"))
    .mockResolvedValueOnce(available("new"));
  await cachedRead("a", ["x"], load, signal);
  invalidate("recording", "r1", { liked: true });
  expect((await cachedRead("a", ["x"], load, signal)).value).toBe("old");
  expect(load).toHaveBeenCalledTimes(1);
  invalidate("catalog");
  expect((await cachedRead("a", ["x"], load, signal)).value).toBe("old");
  await waitFor(() => expect(load).toHaveBeenCalledTimes(2));
  await waitFor(async () =>
    expect((await cachedRead("a", ["x"], load, signal)).value).toBe("new"),
  );
});
