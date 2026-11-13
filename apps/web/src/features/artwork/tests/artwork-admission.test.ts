import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { ArtworkRetrieval } from "../api/artworkRetrieval";

type Answer = "busy" | "accept" | "quota" | "session" | "plain429";

const server = {
  answer: "busy" as Answer | ((id: string) => Answer),
  calls: [] as string[],
};

function job(id: string, status: string) {
  return {
    id: `job-${id}`,
    release_id: id,
    status,
    attempts: 0,
    created_at: 1,
    updated_at: 1,
  };
}

function serve() {
  server.calls = [];
  const fetch = vi.fn(async (path: string, init?: RequestInit) => {
    const method = init?.method ?? "GET";
    server.calls.push(`${method} ${path}`);
    const fetchMatch = /^\/api\/v1\/releases\/([^/]+)\/artwork\/fetch$/.exec(
      path,
    );
    if (path.endsWith("/artwork/availability"))
      return Response.json({ can_fetch: true });
    if (fetchMatch) {
      const id = decodeURIComponent(fetchMatch[1]);
      const answer =
        typeof server.answer === "function" ? server.answer(id) : server.answer;
      if (answer === "busy")
        return Response.json(
          {
            error: "Artwork requests are busy. Try again shortly.",
            code: "artwork_admission_busy",
          },
          { status: 429, headers: { "Retry-After": "2" } },
        );
      if (answer === "plain429")
        return Response.json(
          { error: "Too many requests." },
          { status: 429, headers: { "Retry-After": "2" } },
        );
      if (answer === "quota")
        return Response.json(
          {
            error:
              "Artwork storage is full. Existing artwork has been retained.",
          },
          { status: 409 },
        );
      if (answer === "session")
        return Response.json(
          { error: "Artwork is already being retrieved by another session" },
          { status: 409 },
        );
      return Response.json(job(id, "queued"), { status: 202 });
    }
    const receipt = /^\/api\/v1\/release-artwork-fetches\/job-([^/]+)$/.exec(
      path,
    );
    if (receipt) return Response.json(job(receipt[1], "queued"));
    throw new Error(`Unexpected request ${method} ${path}`);
  });
  vi.stubGlobal("fetch", fetch);
}

const posts = (id?: string) =>
  server.calls.filter((call) =>
    id
      ? call === `POST /api/v1/releases/${id}/artwork/fetch`
      : call.startsWith("POST ") && call.endsWith("/artwork/fetch"),
  );
const receiptReads = (id: string) =>
  server.calls.filter(
    (call) => call === `GET /api/v1/release-artwork-fetches/job-${id}`,
  );

function cover(retrieval: ArtworkRetrieval, id: string) {
  const target = { kind: "release" as const, id };
  const stop = retrieval.subscribe(target, () => {});
  retrieval.missing(target);
  return { target, stop };
}

const flush = (ms = 0) => vi.advanceTimersByTimeAsync(ms);

beforeEach(() => {
  vi.useFakeTimers();
  server.answer = "busy";
  serve();
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

test("many distinct covers under admission saturation send only a bounded number of POSTs before the shared cooldown", async () => {
  const retrieval = new ArtworkRetrieval("alice");
  const covers = Array.from({ length: 40 }, (_, index) =>
    cover(retrieval, `album-${index}`),
  );
  await flush(100);
  expect(posts().length).toBeGreaterThan(0);
  expect(posts().length).toBeLessThanOrEqual(2);
  await flush(1500);
  expect(posts().length).toBeLessThanOrEqual(2);
  await flush(1000);
  expect(posts().length).toBeLessThanOrEqual(4);
  for (const { target } of covers.slice(0, 3))
    expect(retrieval.snapshot(target).phase).not.toBe("failed");
  for (const { stop } of covers) stop();
  retrieval.close();
});

test("admission resumes after Retry-After with the same target", async () => {
  const retrieval = new ArtworkRetrieval("alice");
  const { target, stop } = cover(retrieval, "album-a");
  await flush(100);
  expect(posts("album-a")).toHaveLength(1);
  expect(retrieval.snapshot(target).phase).toBe("waiting");
  server.answer = "accept";
  await flush(1500);
  expect(posts("album-a")).toHaveLength(1);
  await flush(600);
  expect(posts("album-a")).toHaveLength(2);
  expect(retrieval.snapshot(target).phase).toBe("queued");
  stop();
  retrieval.close();
});

test("receipts already accepted keep polling while new admission cools down", async () => {
  server.answer = (id) => (id === "accepted" ? "accept" : "busy");
  const retrieval = new ArtworkRetrieval("alice");
  const accepted = cover(retrieval, "accepted");
  await flush(50);
  expect(posts("accepted")).toHaveLength(1);
  const rejected = cover(retrieval, "rejected");
  await flush(50);
  expect(posts("rejected")).toHaveLength(1);
  await flush(1100);
  expect(receiptReads("accepted").length).toBeGreaterThan(0);
  expect(posts("rejected")).toHaveLength(1);
  accepted.stop();
  rejected.stop();
  retrieval.close();
});

test("a 409 for storage or another session and an untyped 429 stay terminal", async () => {
  for (const answer of ["quota", "session", "plain429"] as const) {
    server.answer = answer;
    serve();
    const retrieval = new ArtworkRetrieval("alice");
    const { target, stop } = cover(retrieval, `album-${answer}`);
    await flush(100);
    expect(retrieval.snapshot(target).phase).toBe("failed");
    await flush(10_000);
    expect(posts()).toHaveLength(1);
    stop();
    retrieval.close();
  }
});

test("a target that stays unadmitted is retried a finite number of times and then fails", async () => {
  const retrieval = new ArtworkRetrieval("alice");
  const { target, stop } = cover(retrieval, "album-a");
  await flush(60_000);
  const sent = posts("album-a").length;
  expect(sent).toBeGreaterThan(1);
  expect(sent).toBeLessThanOrEqual(5);
  expect(retrieval.snapshot(target).phase).toBe("failed");
  await flush(60_000);
  expect(posts("album-a")).toHaveLength(sent);
  stop();
  retrieval.close();
});

test("an unmount during the cooldown sends no late retry", async () => {
  const retrieval = new ArtworkRetrieval("alice");
  const { stop } = cover(retrieval, "album-a");
  await flush(100);
  expect(posts("album-a")).toHaveLength(1);
  stop();
  server.answer = "accept";
  await flush(10_000);
  expect(posts("album-a")).toHaveLength(1);
  retrieval.close();
});

test("an account switch during the cooldown sends no late retry for the old account", async () => {
  const retrieval = new ArtworkRetrieval("alice");
  const { target } = cover(retrieval, "album-a");
  await flush(100);
  expect(posts("album-a")).toHaveLength(1);
  retrieval.close();
  server.answer = "accept";
  await flush(10_000);
  expect(posts("album-a")).toHaveLength(1);
  expect(retrieval.snapshot(target).phase).toBe("idle");
});
