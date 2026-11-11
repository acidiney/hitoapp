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

import { rankLyricCandidates } from "../model/lyricMatch";

const row = (id: string, title: string, artist: string, ms: number | null) => ({
  ...candidate(id, title),
  artist,
  duration_ms: ms,
});

test("candidates with the same normalised title, artist and rounded duration collapse to the first one, keeping its exact id", () => {
  const ranked = rankLyricCandidates(
    [
      row("first", "EoO", "Bad Bunny", 205_200),
      row("second", "eoo", "BAD BUNNY", 205_400),
      row("third", "EoO (Live)", "Bad Bunny", 205_300),
      row("other-length", "EoO", "Bad Bunny", 212_000),
    ],
    { title: "EoO", artist: "Bad Bunny", durationMs: 205_000 },
  );
  expect(ranked.map((item) => item.id)).toEqual(["first", "other-length"]);
});

test("exact title and artist matches come first by closest duration, then the rest, and noisy titles sink within each group", () => {
  const ranked = rankLyricCandidates(
    [
      row("different-song", "Another Song", "Bad Bunny", 205_000),
      row("visualizer", "EoO (Visualizer)", "Bad Bunny", 205_000),
      row("far", "EoO", "Bad Bunny", 209_000),
      row("near", "EoO", "Bad Bunny", 205_500),
      row(
        "long-title",
        "BAD BUNNY - EoO (Visualizer) | DeBÍ TiRAR MáS FOToS",
        "Bad Bunny",
        205_000,
      ),
      row("live-song", "Another Song Live", "Bad Bunny", 205_000),
    ],
    { title: "EoO", artist: "Bad Bunny", durationMs: 205_000 },
  );
  expect(ranked.map((item) => item.id)).toEqual([
    "near",
    "far",
    "visualizer",
    "different-song",
    "long-title",
    "live-song",
  ]);
});

test("ranking never alters or drops the ids it keeps", () => {
  const input = [row("a", "One", "X", null), row("b", "Two", "Y", 1000)];
  const ranked = rankLyricCandidates(input, {
    title: "",
    artist: "",
    durationMs: null,
  });
  expect(ranked.map((item) => item.id).sort()).toEqual(["a", "b"]);
  expect(ranked[0]).toBe(input.find((item) => item.id === ranked[0].id));
});
