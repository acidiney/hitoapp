import { afterEach, expect, test, vi } from "vitest";
import {
  artistGroupTracksListing,
  artistTracksListing,
} from "../api/trackListings";

afterEach(() => {
  vi.unstubAllGlobals();
  window.history.replaceState(null, "", "/");
});

const track = {
  id: "r",
  title: "T",
  artist: "A",
  duration_ms: 1,
  available: true,
};

test("artist pages ask for later track pages with the track offset", async () => {
  const fetch = vi.fn(async () =>
    Response.json({ recordings: { total: 9, items: [track] } }),
  );
  vi.stubGlobal("fetch", fetch);
  const listing = artistTracksListing("account", "a/b", {
    total: 9,
    offset: 0,
  });
  const page = await listing.fetch(200, 200, new AbortController().signal);
  expect(fetch.mock.calls[0][0]).toBe(
    "/api/v1/artists/a%2Fb?recording_offset=200&limit=200",
  );
  expect(page).toEqual({ total: 9, items: [track] });
});

test("chosen artist pages keep their library filter and unwrap members", async () => {
  window.history.replaceState(null, "", "/artist-groups/g?library=main");
  const fetch = vi.fn(async () =>
    Response.json({ recordings: { total: 5, items: [{ recording: track }] } }),
  );
  vi.stubGlobal("fetch", fetch);
  const listing = artistGroupTracksListing("account", "g", {
    total: 5,
    offset: 0,
  });
  const page = await listing.fetch(2, 200, new AbortController().signal);
  expect(fetch.mock.calls[0][0]).toBe(
    "/api/v1/artist-identity-groups/g?library=main&recording_offset=2&limit=200",
  );
  expect(page.items).toEqual([track]);
});
