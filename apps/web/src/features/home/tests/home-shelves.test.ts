import { expect, test } from "vitest";
import type { Schema } from "../../../shared/schema";
import { addedShelf, artistShelf } from "../model/homeShelves";

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

test("chosen artists show their cover recording and track count", () => {
  const items: Schema<"ArtistIdentityGroup">[] = [
    {
      id: "group/1",
      provider: { kind: "builtin_musicbrainz" },
      provider_artist_id: "mb",
      name: {
        artist_id: "a",
        association_id: "x",
        recording_id: "rec-1",
        value: "Slipknot",
      },
      library_count: 1,
      recording_count: 1,
      release_count: 1,
    },
  ];
  const [card] = artistShelf(items);
  expect(card.href).toBe("/artist-groups/group%2F1");
  expect(card.title).toBe("Slipknot");
  expect(card.subtitle).toBe("1 track");
  expect(card.cover).toEqual({
    kind: "artist",
    id: "group/1",
    recording: "rec-1",
  });
});
