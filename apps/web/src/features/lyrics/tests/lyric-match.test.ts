import { expect, test } from "vitest";
import { confidentMatch, normalizeLyricText } from "../model/lyricMatch";
import { candidate } from "../fixtures/lyricIdentityWire";

const facts = {
  title: "Slow Tide",
  artist: "Nadia Ferreira",
  durationMs: 200_000,
};
const hit = (over: Partial<ReturnType<typeof candidate>>) => ({
  ...candidate("x", "Slow Tide"),
  artist: "Nadia Ferreira",
  duration_ms: 200_500,
  ...over,
});

test("normalisation ignores case, accents, punctuation and feat or bracket suffixes", () => {
  expect(
    normalizeLyricText("  Maré  Vem—Devagar!! (Live) [2011 Remaster]"),
  ).toBe("mare vem devagar");
  expect(normalizeLyricText("Song feat. Someone Else")).toBe("song");
  expect(normalizeLyricText("Song ft Someone")).toBe("song");
  expect(normalizeLyricText("Song (feat. Other)")).toBe("song");
});

test("a candidate with the same title, an equal or containing artist and a close duration is confident", () => {
  expect(confidentMatch([hit({})], facts)?.id).toBe("x");
  expect(
    confidentMatch(
      [
        hit({
          title: "SLOW TIDE (feat. Guest)",
          artist: "nadia ferreira & band",
        }),
      ],
      facts,
    ),
  ).not.toBeNull();
  expect(confidentMatch([hit({ artist: "Nadia" })], facts)).not.toBeNull();
  expect(
    confidentMatch([hit({ duration_ms: undefined })], facts),
  ).not.toBeNull();
  expect(
    confidentMatch([hit({})], { ...facts, durationMs: null }),
  ).not.toBeNull();
});

test("weak matches are never confident", () => {
  expect(confidentMatch([hit({ title: "Slow Tides" })], facts)).toBeNull();
  expect(confidentMatch([hit({ title: "Slow" })], facts)).toBeNull();
  expect(confidentMatch([hit({ artist: "Someone Else" })], facts)).toBeNull();
  expect(confidentMatch([hit({ artist: "" })], facts)).toBeNull();
  expect(confidentMatch([hit({ duration_ms: 203_000 })], facts)).toBeNull();
  expect(confidentMatch([], facts)).toBeNull();
  expect(confidentMatch([hit({})], { ...facts, title: "" })).toBeNull();
});

test("the closest duration wins among confident candidates", () => {
  const picked = confidentMatch(
    [
      hit({ id: "far", duration_ms: 201_800 }),
      hit({ id: "near", duration_ms: 200_100 }),
    ],
    facts,
  );
  expect(picked?.id).toBe("near");
});
