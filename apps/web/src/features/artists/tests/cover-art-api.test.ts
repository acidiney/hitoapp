import { expect, test } from "vitest";
import { coverArtUrl } from "../api/coverArtApi";

const id = "6c1e3a2e-6b0d-4a52-8d58-0b6a5d6f7a10";

test("each cover kind builds its own path under the cover-art prefix", () => {
  expect(coverArtUrl("release", id)).toBe(`/api/v1/cover-art/release/${id}`);
  expect(coverArtUrl("release-group", id)).toBe(
    `/api/v1/cover-art/release-group/${id}`,
  );
  expect(coverArtUrl("artist", id)).toBe(`/api/v1/cover-art/artist/${id}`);
  expect(coverArtUrl("credited-artist", id)).toBe(
    `/api/v1/cover-art/credited-artist/${id}`,
  );
});

test("the identifier is escaped as one path segment", () => {
  expect(coverArtUrl("credited-artist", "a/b c")).toBe(
    "/api/v1/cover-art/credited-artist/a%2Fb%20c",
  );
});
