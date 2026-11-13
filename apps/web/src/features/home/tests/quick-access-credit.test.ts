import { expect, test } from "vitest";
import type { Schema } from "../../../shared/schema";
import { quickAccess } from "../model/quickAccess";

function home(
  albums: Pick<Schema<"HomeAlbum">, "artist" | "inferred_artist">[],
): Schema<"Home"> {
  return {
    has_music: true,
    liked_count: 0,
    unheard: [],
    recently_added: [],
    recent_albums: albums.map((album, index) => ({
      id: `album-${index}`,
      title: `Album ${index}`,
      last_played_at: 10 - index,
      played: 1,
      ...album,
    })),
  };
}

test("an album without its own artist shows the server's inferred credit", () => {
  const tiles = quickAccess(
    home([
      { artist: null, inferred_artist: { kind: "named", name: "Same Band" } },
      { artist: null, inferred_artist: { kind: "various" } },
      { artist: null, inferred_artist: null },
      { artist: "Owner", inferred_artist: null },
    ]),
    [],
  );
  expect(tiles.map((tile) => tile.subtitle)).toEqual([
    "Album · Same Band",
    "Album · Various artists",
    "Album",
    "Album · Owner",
  ]);
  expect(tiles.map((tile) => tile.coverArtist)).toEqual([
    "Same Band",
    "Various artists",
    undefined,
    "Owner",
  ]);
});

test("an album tile encodes its id in the album link", () => {
  const data = home([{ artist: "Owner", inferred_artist: null }]);
  data.recent_albums[0].id = "a/b c?d";
  expect(quickAccess(data, [])[0].href).toBe("/albums/a%2Fb%20c%3Fd");
});
