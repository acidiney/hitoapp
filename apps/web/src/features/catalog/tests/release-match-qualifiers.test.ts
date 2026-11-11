import type { Schema } from "../../../shared/schema";
import { expect, test } from "vitest";
import { matchReleases } from "../model/releaseMatch";

function group(title: string, id = title): Schema<"CatalogReleaseGroup"> {
  return {
    id,
    title,
    kind: "single",
    secondary_types: [],
    first_release_date: "2017-01-01",
    artist_credit: null,
  } as unknown as Schema<"CatalogReleaseGroup">;
}

const original = group("Escápate conmigo");

test("a remix title never matches the original release", () => {
  const remix =
    "Escápate Conmigo (feat. Ozuna, Bad Bunny, De La Ghetto, Arcángel, Noriel & Almighty) - Remix";
  expect(matchReleases([original], remix, 2017).suggestion).toBeNull();
});

test("an original title never matches a remix release", () => {
  const remix = group("Escápate conmigo - Remix", "remix");
  expect(
    matchReleases([remix], "Escápate conmigo", 2017).suggestion,
  ).toBeNull();
});

test.each([
  "Song (Live)",
  "Song (Acoustic)",
  "Song (Remastered 2011)",
  "Song - Radio Edit",
  "Song (Instrumental)",
  "Song (Sped Up)",
  "Song (Slowed)",
  "Song (Karaoke Version)",
  "Song (feat. Someone)",
])("%s is a different version from Song", (title) => {
  expect(matchReleases([group("Song")], title, null).suggestion).toBeNull();
  expect(matchReleases([group(title)], "Song", null).suggestion).toBeNull();
});

test("the same qualifiers still match, ignoring case, accents and brackets style", () => {
  const found = matchReleases([group("Song [Live]")], "SONG (live)", null);
  expect(found.suggestion?.id).toBe("Song [Live]");
  expect(found.unique).toBe(true);
});

test("a plain title still matches a plain release", () => {
  expect(matchReleases([original], "Escápate Conmigo", 2017).suggestion).toBe(
    original,
  );
});
