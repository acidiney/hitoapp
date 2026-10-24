import { afterEach, expect, test, vi } from "vitest";
import { request } from "../http";

afterEach(() => vi.restoreAllMocks());

async function events(action: string) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => new Response("{}")),
  );
  const info = vi.spyOn(console, "info").mockImplementation(() => {});
  await request("/x", { action });
  vi.unstubAllGlobals();
  return info.mock.calls.filter(
    ([, fields]) => fields?.event === "http.response",
  );
}

test("an ordinary accepted request logs one info line", async () => {
  expect(await events("catalog.something")).toHaveLength(1);
});

test("queue page fetches, which can number in the hundreds, stay below info", async () => {
  expect(await events("catalog.tracks_page")).toHaveLength(0);
  expect(await events("catalog.album_tracks_page")).toHaveLength(0);
  expect(await events("catalog.artist_tracks_page")).toHaveLength(0);
  expect(await events("catalog.artist_group_tracks_page")).toHaveLength(0);
});
