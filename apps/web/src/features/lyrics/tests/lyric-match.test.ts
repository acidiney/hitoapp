import { expect, test } from "vitest";
import { normalizeLyricText, rankLyricCandidates } from "../model/lyricMatch";
import { candidate } from "../fixtures/lyricIdentityWire";

test("normalisation ignores case, accents, punctuation and feat or bracket suffixes", () => {
  expect(
    normalizeLyricText("  Maré  Vem—Devagar!! (Live) [2011 Remaster]"),
  ).toBe("mare vem devagar");
  expect(normalizeLyricText("Song feat. Someone Else")).toBe("song");
  expect(normalizeLyricText("Song ft Someone")).toBe("song");
  expect(normalizeLyricText("Song (feat. Other)")).toBe("song");
});

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
