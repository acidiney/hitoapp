import { afterEach, beforeEach, expect, test, vi } from "vitest";
import {
  connectionRestoredEvent,
  getConnection,
  reportReachable,
  reportUnreachable,
  resetConnection,
  retryConnectionNow,
  subscribeConnection,
  watchServer,
} from "../connection";

function serverState(up: { value: boolean }) {
  return vi.fn(async (_url: unknown, _init?: unknown) => {
    if (!up.value) throw new TypeError("Failed to fetch");
    return new Response("{}", { status: 200 });
  });
}

beforeEach(() => {
  vi.useFakeTimers();
  resetConnection();
});
afterEach(() => resetConnection());

test("a reported outage is visible with its reason and a success clears it", () => {
  const seen = vi.fn();
  subscribeConnection(seen);
  expect(getConnection()).toEqual({ reachable: true });
  reportUnreachable("gateway");
  expect(getConnection()).toMatchObject({
    reachable: false,
    reason: "gateway",
  });
  reportReachable();
  expect(getConnection()).toEqual({ reachable: true });
  expect(seen).toHaveBeenCalledTimes(2);
});

test("repeated reports of the same outage notify once and keep the first reason", () => {
  const seen = vi.fn();
  subscribeConnection(seen);
  reportUnreachable("network");
  reportUnreachable("timeout");
  expect(getConnection()).toMatchObject({ reason: "network" });
  expect(seen).toHaveBeenCalledTimes(1);
});

test("probing starts at once, backs off, and restores without any user action", async () => {
  const up = { value: false };
  const fetchMock = serverState(up);
  vi.stubGlobal("fetch", fetchMock);
  const restored = vi.fn();
  window.addEventListener(connectionRestoredEvent, restored);
  reportUnreachable("network");
  await vi.advanceTimersByTimeAsync(2_000);
  expect(fetchMock).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(4_000);
  expect(fetchMock).toHaveBeenCalledTimes(2);
  expect(getConnection().reachable).toBe(false);
  up.value = true;
  await vi.advanceTimersByTimeAsync(8_000);
  expect(getConnection()).toEqual({ reachable: true });
  expect(restored).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(60_000);
  expect(fetchMock).toHaveBeenCalledTimes(3);
  window.removeEventListener(connectionRestoredEvent, restored);
});

test("the wait between probes is capped", async () => {
  const fetchMock = serverState({ value: false });
  vi.stubGlobal("fetch", fetchMock);
  reportUnreachable("network");
  await vi.advanceTimersByTimeAsync(2_000 + 4_000 + 8_000 + 15_000);
  expect(fetchMock).toHaveBeenCalledTimes(4);
  await vi.advanceTimersByTimeAsync(15_000);
  expect(fetchMock).toHaveBeenCalledTimes(5);
});

test("a server that answers the health check as not ready stays unavailable", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => new Response('{"ready":false}', { status: 503 })),
  );
  reportUnreachable("gateway");
  await vi.advanceTimersByTimeAsync(2_000);
  expect(getConnection().reachable).toBe(false);
});

test("a probe that never answers is abandoned and the next one still runs", async () => {
  const fetchMock = vi.fn(
    (_url: unknown, init?: { signal?: AbortSignal }) =>
      new Promise<Response>((_, reject) =>
        init?.signal?.addEventListener("abort", () =>
          reject(new DOMException("aborted", "AbortError")),
        ),
      ),
  );
  vi.stubGlobal("fetch", fetchMock);
  reportUnreachable("timeout");
  await vi.advanceTimersByTimeAsync(2_000 + 5_000);
  await vi.advanceTimersByTimeAsync(4_000);
  expect(fetchMock).toHaveBeenCalledTimes(2);
});

test("retrying now probes immediately and regaining the network probes at once", async () => {
  const up = { value: true };
  const fetchMock = serverState(up);
  vi.stubGlobal("fetch", fetchMock);
  reportUnreachable("network");
  retryConnectionNow();
  await vi.advanceTimersByTimeAsync(0);
  expect(getConnection().reachable).toBe(true);
  expect(fetchMock).toHaveBeenCalledTimes(1);
  up.value = false;
  reportUnreachable("network");
  up.value = true;
  window.dispatchEvent(new Event("online"));
  await vi.advanceTimersByTimeAsync(0);
  expect(getConnection().reachable).toBe(true);
});

test("retrying now while a probe is in flight probes again as soon as it fails", async () => {
  let release: (response: Response) => void = () => {};
  const fetchMock = vi
    .fn()
    .mockImplementationOnce(
      () => new Promise<Response>((resolve) => (release = resolve)),
    )
    .mockResolvedValue(new Response("{}", { status: 200 }));
  vi.stubGlobal("fetch", fetchMock);
  reportUnreachable("network");
  await vi.advanceTimersByTimeAsync(2_000);
  expect(fetchMock).toHaveBeenCalledTimes(1);
  retryConnectionNow();
  release(new Response("", { status: 503 }));
  await vi.advanceTimersByTimeAsync(0);
  expect(fetchMock).toHaveBeenCalledTimes(2);
  expect(getConnection().reachable).toBe(true);
});

test("probes never carry credentials, bodies or the request that failed", async () => {
  const fetchMock = serverState({ value: true });
  vi.stubGlobal("fetch", fetchMock);
  reportUnreachable("network");
  await vi.advanceTimersByTimeAsync(2_000);
  const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
  expect(url).toBe("/health");
  expect(init.body).toBeUndefined();
  expect(init.cache).toBe("no-store");
});

test("outage and recovery log only a reason, a count and a duration", async () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const info = vi.spyOn(console, "info").mockImplementation(() => {});
  vi.stubGlobal("fetch", serverState({ value: true }));
  reportUnreachable("network");
  await vi.advanceTimersByTimeAsync(2_000);
  expect(warn.mock.calls[0]?.[1]).toMatchObject({
    event: "connection.lost",
    reason: "network",
  });
  const restored = info.mock.calls.find(
    ([, fields]) => fields?.event === "connection.restored",
  );
  expect(restored?.[1]).toMatchObject({ count: 1 });
  expect(typeof restored?.[1]?.duration_ms).toBe("number");
});

test("watching the server fires once when it returns and stops when released", async () => {
  const up = { value: false };
  vi.stubGlobal("fetch", serverState(up));
  const returned = vi.fn();
  const stop = watchServer(returned);
  await vi.advanceTimersByTimeAsync(2_000);
  expect(returned).not.toHaveBeenCalled();
  up.value = true;
  await vi.advanceTimersByTimeAsync(4_000);
  expect(returned).toHaveBeenCalledTimes(1);
  stop();
  reportUnreachable("network");
  await vi.advanceTimersByTimeAsync(2_000);
  expect(returned).toHaveBeenCalledTimes(1);
});
