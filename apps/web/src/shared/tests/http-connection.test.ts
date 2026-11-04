import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { HttpError, request } from "../http";
import { getConnection, resetConnection } from "../connection";
import { errorMessage } from "../format";

beforeEach(() => {
  resetConnection();
  vi.spyOn(console, "warn").mockImplementation(() => {});
});
afterEach(() => resetConnection());

const fail = (action = "catalog.read", signal?: AbortSignal) =>
  request("/secret/path?q=private-search", { action, signal }).catch(
    (error: unknown) => error,
  );

test("a network failure becomes a plain message and marks the server unreachable", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn().mockRejectedValue(new TypeError("Failed to fetch")),
  );
  const error = await fail();
  expect(error).toBeInstanceOf(TypeError);
  const message = errorMessage(error);
  expect(message).toBe(
    "Hito cannot be reached right now. It will keep trying.",
  );
  expect(message).not.toMatch(/fetch|private-search|secret/i);
  expect(getConnection()).toMatchObject({
    reachable: false,
    reason: "network",
  });
});

test("a cancelled request is not an outage", async () => {
  const controller = new AbortController();
  controller.abort();
  vi.stubGlobal(
    "fetch",
    vi.fn().mockRejectedValue(new DOMException("x", "AbortError")),
  );
  await fail("catalog.read", controller.signal);
  expect(getConnection().reachable).toBe(true);
});

test("a request that never answers times out as an outage", async () => {
  vi.useFakeTimers();
  vi.stubGlobal(
    "fetch",
    vi.fn(
      (_url: unknown, init: RequestInit) =>
        new Promise((_, reject) =>
          init.signal?.addEventListener("abort", () =>
            reject(new DOMException("x", "AbortError")),
          ),
        ),
    ),
  );
  const pending = request("/slow", { action: "catalog.read", timeoutMs: 1000 });
  const outcome = pending.catch((error: unknown) => error);
  await vi.advanceTimersByTimeAsync(1000);
  const error = await outcome;
  expect(errorMessage(error)).toBe(
    "Hito is taking too long to answer. It will keep trying.",
  );
  expect(getConnection()).toMatchObject({
    reachable: false,
    reason: "timeout",
  });
  vi.useRealTimers();
});

test("the caller's own cancellation still works while a timeout is armed", async () => {
  const controller = new AbortController();
  const seen = vi.fn((_url: unknown, init: RequestInit) => {
    expect(init.signal?.aborted).toBe(false);
    controller.abort();
    expect(init.signal?.aborted).toBe(true);
    return Promise.reject(new DOMException("x", "AbortError"));
  });
  vi.stubGlobal("fetch", seen);
  await fail("catalog.read", controller.signal);
  expect(getConnection().reachable).toBe(true);
});

test.each([502, 504])(
  "a %i from a proxy is an outage with a plain message and no body",
  async (status) => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response("<html>nginx 502 Bad Gateway 10.0.0.7</html>", {
          status,
        }),
      ),
    );
    const error = await fail();
    expect(error).toBeInstanceOf(HttpError);
    expect((error as HttpError).status).toBe(status);
    expect(errorMessage(error)).toBe(
      "Hito is not responding right now. It will keep trying.",
    );
    expect(getConnection()).toMatchObject({
      reachable: false,
      reason: "gateway",
    });
  },
);

test("a bare 503 is an outage but a 503 or 502 from Hito itself is not", async () => {
  vi.stubGlobal(
    "fetch",
    vi
      .fn()
      .mockResolvedValue(new Response("Service Unavailable", { status: 503 })),
  );
  expect(errorMessage(await fail())).toBe(
    "Hito is not responding right now. It will keep trying.",
  );
  expect(getConnection().reachable).toBe(false);
  for (const status of [502, 503, 504]) {
    resetConnection();
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify({ error: "Provider is busy" }), {
          status,
        }),
      ),
    );
    expect(errorMessage(await fail())).toBe("Provider is busy");
    expect(getConnection().reachable).toBe(true);
  }
});

test("an ordinary application error leaves the connection alone", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn().mockResolvedValue(
      new Response(JSON.stringify({ error: "Nope", code: "x" }), {
        status: 422,
      }),
    ),
  );
  await fail();
  expect(getConnection().reachable).toBe(true);
});

test("any answer from the server ends an outage", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn().mockRejectedValueOnce(new TypeError("Failed to fetch")),
  );
  await fail();
  expect(getConnection().reachable).toBe(false);
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response("{}")));
  await request("/ok", { action: "catalog.read" });
  expect(getConnection().reachable).toBe(true);
});

test("a failure whose body the caller skips is not classified as an outage", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn().mockResolvedValue(new Response("x", { status: 502 })),
  );
  await request("/x", {
    action: "community.playback.report",
    errorBody: false,
  }).catch(() => {});
  expect(getConnection().reachable).toBe(true);
});
