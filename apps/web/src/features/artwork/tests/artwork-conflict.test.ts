import { afterEach, expect, test, vi } from "vitest";
import { ArtworkRetrieval } from "../api/artworkRetrieval";

const settle = () => new Promise((resolve) => setTimeout(resolve, 20));

function serveConflict() {
  const calls: string[] = [];
  const fetch = vi.fn(async (path: string, init?: RequestInit) => {
    calls.push(`${init?.method ?? "GET"} ${path}`);
    if (path.endsWith("/availability"))
      return Response.json({ can_fetch: true });
    return Response.json(
      {
        error:
          "Artwork requests are full. Try again after pending requests finish.",
      },
      { status: 409 },
    );
  });
  vi.stubGlobal("fetch", fetch);
  return calls;
}

const posts = (calls: string[], id: string) =>
  calls.filter((call) => call === `POST /api/v1/releases/${id}/artwork/fetch`);

afterEach(() => vi.unstubAllGlobals());

test("a 409 answer is fetched once per id and repeated missing covers do not retry it", async () => {
  const calls = serveConflict();
  const retrieval = new ArtworkRetrieval("alice");
  const target = { kind: "release" as const, id: "album-1" };
  const stop = retrieval.subscribe(target, () => {});
  for (let attempt = 0; attempt < 5; attempt++) {
    retrieval.missing(target, retrieval.snapshot(target).revision);
    await settle();
  }
  expect(posts(calls, "album-1")).toHaveLength(1);
  expect(retrieval.snapshot(target).phase).toBe("failed");
  stop();
  const again = retrieval.subscribe(target, () => {});
  retrieval.missing(target, retrieval.snapshot(target).revision);
  await settle();
  expect(posts(calls, "album-1")).toHaveLength(1);
  again();
});

test("a remounted cover keeps its settled 409 even after more than 128 other covers settle", async () => {
  const calls = serveConflict();
  const retrieval = new ArtworkRetrieval("alice");
  const first = { kind: "release" as const, id: "album-0" };
  const stopFirst = retrieval.subscribe(first, () => {});
  retrieval.missing(first);
  await settle();
  const stops = [];
  for (let index = 1; index <= 140; index++) {
    const target = { kind: "release" as const, id: `album-${index}` };
    stops.push(retrieval.subscribe(target, () => {}));
    retrieval.missing(target);
  }
  for (let round = 0; round < 20; round++) await settle();
  stopFirst();
  const remounted = retrieval.subscribe(first, () => {});
  retrieval.missing(first, retrieval.snapshot(first).revision);
  await settle();
  expect(posts(calls, "album-0")).toHaveLength(1);
  remounted();
  for (const stop of stops) stop();
});
