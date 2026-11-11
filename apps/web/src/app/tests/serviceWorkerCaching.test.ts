import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { runInNewContext } from "node:vm";
import { beforeEach, expect, test, vi } from "vitest";

type Handler = (event: unknown) => void;

const origin = "https://hito.test";
const source = readFileSync(resolve(process.cwd(), "public/sw.js"), "utf8");

function harness() {
  const handlers = new Map<string, Handler>();
  const stores = new Map<string, Map<string, Response>>();
  const puts: string[] = [];
  const key = (request: unknown) =>
    typeof request === "string" ? request : (request as { url: string }).url;
  const open = async (name: string) => {
    const store = stores.get(name) ?? new Map<string, Response>();
    stores.set(name, store);
    return {
      match: async (request: unknown) =>
        store.get(key(request).replace(origin, "")) ?? store.get(key(request)),
      put: async (request: unknown, response: Response) => {
        puts.push(key(request));
        store.set(key(request).replace(origin, ""), response);
      },
      add: async (request: string) => {
        puts.push(request);
        store.set(request, new Response("x"));
      },
    };
  };
  const caches = {
    open: vi.fn(open),
    keys: async () => [...stores.keys()],
    delete: vi.fn(async (name: string) => stores.delete(name)),
  };
  const self = {
    location: { origin },
    addEventListener: (name: string, handler: Handler) =>
      handlers.set(name, handler),
    skipWaiting: vi.fn(),
    clients: { claim: vi.fn().mockResolvedValue(undefined) },
  };
  const fetchMock = vi.fn();
  runInNewContext(source, {
    self,
    caches,
    fetch: fetchMock,
    Response,
    URL,
    setTimeout,
    clearTimeout,
    Promise,
    Error,
  });
  return { handlers, stores, puts, caches, self, fetchMock };
}

function request(
  path: string,
  options: {
    method?: string;
    mode?: string;
    headers?: Record<string, string>;
    destination?: string;
  } = {},
) {
  return {
    url: path.startsWith("http") ? path : origin + path,
    method: options.method ?? "GET",
    mode: options.mode ?? "no-cors",
    destination: options.destination ?? "",
    headers: new Headers(options.headers),
  };
}

async function dispatch(
  h: ReturnType<typeof harness>,
  req: ReturnType<typeof request>,
) {
  let response: Promise<Response> | undefined;
  h.handlers.get("fetch")?.({
    request: req,
    respondWith: (value: Promise<Response>) => (response = value),
  });
  return response;
}

let h: ReturnType<typeof harness>;
function basicResponse(body: string, status = 200) {
  const response = new Response(body, { status });
  Object.defineProperty(response, "type", { value: "basic" });
  return response;
}

beforeEach(() => {
  h = harness();
  h.fetchMock.mockImplementation(async () => basicResponse("network"));
});

test.each([
  ["an API read", request("/api/v1/recordings")],
  ["an API write", request("/api/v1/login", { method: "POST" })],
  [
    "an API read with an authorization header",
    request("/api/v1/me", { headers: { authorization: "Bearer x" } }),
  ],
  ["audio", request("/api/v1/recordings/1/audio", { destination: "audio" })],
  [
    "audio by range",
    request("/assets/song-abcdef12.png", { headers: { range: "bytes=0-1" } }),
  ],
  ["a streamed audio URL", request("/api/v1/stream/1?signature=abc")],
  ["artwork with a signed URL", request("/api/v1/artwork/1?sig=abc&expires=9")],
  [
    "a hashed asset with a signed query",
    request("/assets/cover-abcdef12.png?sig=abc"),
  ],
  [
    "an asset with an authorization header",
    request("/assets/app-abcdef12.js", { headers: { authorization: "x" } }),
  ],
  [
    "a hashed asset sent as a POST",
    request("/assets/app-abcdef12.js", { method: "POST" }),
  ],
  ["another origin", request("https://other.test/assets/app-abcdef12.js")],
  ["the unhashed app script", request("/assets/app.js")],
  [
    "a Hito page data request",
    request("/home", { headers: { "x-inertia": "true" } }),
  ],
])("%s is never handled or stored by the service worker", async (_, req) => {
  expect(await dispatch(h, req)).toBeUndefined();
  expect(h.puts.filter((url) => !url.startsWith("/__hito"))).toEqual([]);
});

test("a navigation to an API path is left to the network", async () => {
  expect(
    await dispatch(h, request("/api/v1/export", { mode: "navigate" })),
  ).toBeUndefined();
});

test("a hashed asset is fetched once and then served from the cache", async () => {
  const req = request("/assets/chunk-abcdef12.js");
  const first = await dispatch(h, req);
  expect(await (await first)?.text()).toBe("network");
  const second = await dispatch(h, req);
  expect(await (await second)?.text()).toBe("network");
  expect(h.fetchMock).toHaveBeenCalledTimes(1);
  expect(h.puts).toContain(req.url);
});

test("a partial response is not stored", async () => {
  h.fetchMock.mockResolvedValue(basicResponse("part", 206));
  await await dispatch(h, request("/assets/chunk-abcdef12.js"));
  expect(h.puts).toEqual([]);
});

test("a navigation goes to the network first and its page is never stored", async () => {
  const response = await dispatch(h, request("/home", { mode: "navigate" }));
  expect((await response)?.status).toBe(200);
  expect(h.puts).toEqual([]);
});

test("an offline navigation shows the Hito offline page with Retry", async () => {
  await h.handlers.get("install")?.({ waitUntil: (p: Promise<void>) => p });
  h.fetchMock.mockRejectedValue(new TypeError("offline"));
  const response = await await dispatch(
    h,
    request("/home", { mode: "navigate" }),
  );
  const html = await response?.text();
  expect(html).toContain("Hito can't reach your server");
  expect(html).toContain("Retry");
});

test("a navigation that takes too long falls back to the offline page", async () => {
  vi.useFakeTimers();
  h = harness();
  h.fetchMock.mockReturnValue(new Promise(() => undefined));
  let response: Promise<Response> | undefined;
  h.handlers.get("fetch")?.({
    request: request("/home", { mode: "navigate" }),
    respondWith: (value: Promise<Response>) => (response = value),
  });
  await vi.advanceTimersByTimeAsync(6000);
  expect(await response?.then((value) => value.text())).toContain("Retry");
  vi.useRealTimers();
});

test("install caches only the offline page and icons", async () => {
  let installed: Promise<void> | undefined;
  h.handlers.get("install")?.({
    waitUntil: (p: Promise<void>) => (installed = p),
  });
  await installed;
  expect(
    h.puts.every(
      (url) => url === "/__hito-offline" || /\.(png|svg)$/.test(url),
    ),
  ).toBe(true);
  expect(h.puts.some((url) => url.includes("/api/"))).toBe(false);
});

test("activate removes old caches and takes control", async () => {
  await h.caches.open("hito-static-v0");
  await h.caches.open("hito-static-v1");
  let activated: Promise<void> | undefined;
  h.handlers.get("activate")?.({
    waitUntil: (p: Promise<void>) => (activated = p),
  });
  await activated;
  expect(h.caches.delete).toHaveBeenCalledWith("hito-static-v0");
  expect(h.caches.delete).not.toHaveBeenCalledWith("hito-static-v1");
  expect(h.self.clients.claim).toHaveBeenCalled();
});

test("SKIP_WAITING activates the waiting worker", () => {
  h.handlers.get("message")?.({ data: { type: "SKIP_WAITING" } });
  expect(h.self.skipWaiting).toHaveBeenCalled();
  h.handlers.get("message")?.({ data: { type: "other" } });
  expect(h.self.skipWaiting).toHaveBeenCalledTimes(1);
});
