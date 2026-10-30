import { expect, test } from "vitest";
import { rankArtists, topResultKind } from "../model/ranking";

const artist = (id: string, name: string) =>
  ({ id, name, library_id: "l", recording_count: 1, release_count: 1 }) as never;

test("an exact artist name match comes first, ignoring case and accents", () => {
  const ranked = rankArtists("joeboy", [
    artist("a", "Joeboy and friends"),
    artist("b", "Joéboy"),
    artist("c", "JOEBOY"),
  ]);
  expect(ranked.map((a) => a.id)).toEqual(["b", "c", "a"]);
});

test("the top result is the exact artist even when a track also matches", () => {
  const artists = [artist("c", "Joeboy")];
  expect(topResultKind("joeboy", { tracks: 3, releases: 1, artists })).toBe(
    "artist",
  );
  expect(
    topResultKind("joe", { tracks: 3, releases: 1, artists }),
  ).toBe("track");
  expect(topResultKind("joe", { tracks: 0, releases: 1, artists })).toBe(
    "release",
  );
  expect(topResultKind("joe", { tracks: 0, releases: 0, artists })).toBe(
    "artist",
  );
  expect(
    topResultKind("joe", { tracks: 0, releases: 0, artists: [] }),
  ).toBeNull();
});
