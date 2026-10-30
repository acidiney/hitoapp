import { expect, test } from "vitest";
import { albumCreditLabel } from "../model/albumCredit";
import { quickAccess } from "../../home/model/quickAccess";
import type { Schema } from "../../../shared/schema";

test("a known album artist wins", () => {
  expect(albumCreditLabel("Slipknot", ["Other"])).toBe("Slipknot");
});

test("an unknown album artist uses the one artist every loaded track shares", () => {
  expect(albumCreditLabel(null, ["Slipknot", "Slipknot"])).toBe("Slipknot");
  expect(albumCreditLabel("", ["Slipknot"])).toBe("Slipknot");
});

test("tracks by different artists make the album Various artists", () => {
  expect(albumCreditLabel(null, ["A", "B"])).toBe("Various artists");
});

test("without any track credit nothing is claimed", () => {
  expect(albumCreditLabel(null, [])).toBeNull();
  expect(albumCreditLabel(null, ["", " "])).toBeNull();
});

test("Home quick access drops the separator when the artist is missing", () => {
  const home = {
    liked_count: 0,
    unheard: [],
    recent_albums: [
      { id: "a", title: "No credit", artist: "", last_played_at: 1, played: 1 },
      {
        id: "b",
        title: "Credit",
        artist: "Shore",
        last_played_at: 1,
        played: 1,
      },
    ],
  } as unknown as Schema<"Home">;
  const tiles = quickAccess(home, []);
  expect(tiles[0].subtitle).toBe("Album");
  expect(tiles[1].subtitle).toBe("Album · Shore");
});
