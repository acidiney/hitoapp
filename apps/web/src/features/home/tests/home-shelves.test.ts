import { expect, test } from "vitest";
import type { Schema } from "../../../shared/schema";
import { addedShelf, artistShelf, playedShelf } from "../model/homeShelves";

const played = (id: string, artist: string | null): Schema<"HomeAlbum"> => ({
  id,
  title: `Album ${id}`,
  artist,
  inferred_artist: artist ? null : { kind: "various" },
  last_played_at: 1,
  played: 3,
});

test("recently played albums link to the album with their credit", () => {
  expect(playedShelf([played("a", "Shore"), played("b", null)])).toEqual([
    {
      key: "played-a",
      href: "/albums/a",
      title: "Album a",
      subtitle: "Shore",
      cover: { kind: "release", id: "a", artist: "Shore" },
    },
    {
      key: "played-b",
      href: "/albums/b",
      title: "Album b",
      subtitle: "Various artists",
      cover: { kind: "release", id: "b", artist: "Various artists" },
    },
  ]);
});

test("recently added albums keep the server's order and ids are encoded", () => {
  const cards = addedShelf([
    { id: "x/y", title: "New", artist: "Fresh", inferred_artist: null },
    { id: "z", title: "Older", artist: null, inferred_artist: null },
  ]);
  expect(cards.map((card) => card.href)).toEqual([
    "/albums/x%2Fy",
    "/albums/z",
  ]);
  expect(cards.map((card) => card.subtitle)).toEqual(["Fresh", ""]);
});

const artist = (
  id: string,
  tracks: number,
  albums: number,
  featured?: number,
): Schema<"Artist"> => ({
  id,
  library_id: "library",
  name: `Artist ${id}`,
  recording_count: tracks,
  release_count: albums,
  featured_recording_count: featured,
  artwork_recording_id: `rec-${id}`,
  provider_photo: id === "a/1",
});

test("library artists open the artist page with their cover recording and track count", () => {
  const [card] = artistShelf([artist("a/1", 1, 1)]);
  expect(card.href).toBe("/artists/a%2F1");
  expect(card.title).toBe("Artist a/1");
  expect(card.subtitle).toBe("1 track");
  expect(card.cover).toEqual({
    kind: "artist",
    id: "a/1",
    recording: "rec-a/1",
    photo: true,
  });
});

test("an artist without tracks of its own shows its album count, never zero tracks", () => {
  expect(
    artistShelf([
      artist("tracks", 12, 2),
      artist("albums", 0, 3),
      artist("album", 0, 1),
      artist("guest", 0, 0, 2),
    ]).map((card) => card.subtitle),
  ).toEqual(["12 tracks", "3 albums", "1 album", "2 appearances"]);
});
