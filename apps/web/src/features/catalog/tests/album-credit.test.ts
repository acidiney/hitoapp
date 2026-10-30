import { expect, test } from "vitest";
import { albumCreditLabel, releaseCreditLabel } from "../model/albumCredit";
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

const release = (
  over: Partial<Schema<"Release">>,
): Pick<Schema<"Release">, "artist" | "inferred_artist"> => ({
  artist: null,
  inferred_artist: null,
  ...over,
});

test("the server's named inference is the album credit", () => {
  expect(
    releaseCreditLabel(
      release({ inferred_artist: { kind: "named", name: "Slipknot" } }),
      ["Other"],
    ),
  ).toBe("Slipknot");
});

test("the server's various inference reads Various artists", () => {
  expect(
    releaseCreditLabel(release({ inferred_artist: { kind: "various" } }), [
      "A",
    ]),
  ).toBe("Various artists");
});

test("a stated album artist wins over the inference", () => {
  expect(
    releaseCreditLabel(
      release({
        artist: "Shore",
        inferred_artist: { kind: "named", name: "Other" },
      }),
      [],
    ),
  ).toBe("Shore");
});

test("a null inference claims nothing even when loaded tracks share an artist", () => {
  expect(releaseCreditLabel(release({}), ["Slipknot"])).toBeNull();
});

test("the client fallback applies only where the field is absent", () => {
  const absent = { artist: null } as Pick<
    Schema<"Release">,
    "artist" | "inferred_artist"
  >;
  expect(releaseCreditLabel(absent, ["Slipknot", "Slipknot"])).toBe("Slipknot");
  expect(releaseCreditLabel(absent, ["A", "B"])).toBe("Various artists");
});
