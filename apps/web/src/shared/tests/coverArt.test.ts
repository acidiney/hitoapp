import { expect, test } from "vitest";
import {
  COVER_HUES,
  GENRE_HUES,
  coverArt,
  coverMonogram,
  type CoverArtKind,
} from "../ui/coverArt";
import { contrast } from "../../test/css";

const kinds: CoverArtKind[] = [
  "album",
  "artist",
  "playlist",
  "podcast",
  "audiobook",
  "genre",
  "track",
];

function mix(a: string, b: string, weight: number): string {
  const channels = [1, 3, 5].map((start) => {
    const from = parseInt(a.slice(start, start + 2), 16);
    const to = parseInt(b.slice(start, start + 2), 16);
    return Math.round(from + (to - from) * weight)
      .toString(16)
      .padStart(2, "0");
  });
  return `#${channels.join("")}`;
}

test("the same kind and id always give the same art", () => {
  for (const kind of kinds) {
    const id = "0d2f6a3e-7c2f-4a58-b1d4-1f0c2f4b8a11";
    expect(coverArt(kind, id, "A", "B")).toEqual(coverArt(kind, id, "A", "B"));
  }
});

test("the art depends on the kind, except that a track shares its album's art", () => {
  const album = coverArt("album", "release-1", "T", null);
  expect(coverArt("track", "release-1", "T", null).paint).toEqual(album.paint);
  const other = coverArt("playlist", "release-1", "T", null);
  expect(other.paint.key).not.toBe(album.paint.key);
});

test("every colour is a six digit hex value", () => {
  for (const kind of kinds) {
    const { paint } = coverArt(kind, "release-7", "T", null);
    for (const colour of [
      paint.base,
      paint.deep,
      paint.accent,
      paint.accentSoft,
    ])
      expect(colour).toMatch(/^#[0-9a-f]{6}$/);
  }
});

test("no hue family takes more than 15 percent of 1000 ids, for sequential and random ids", () => {
  const sequential = Array.from(
    { length: 1000 },
    (_, index) => `release-${index}`,
  );
  const random = Array.from(
    { length: 1000 },
    (_, index) =>
      `${(index * 2654435761).toString(16)}-${(index ** 3).toString(36)}-4a58`,
  );
  for (const ids of [sequential, random]) {
    const counts = new Map<number, number>();
    for (const id of ids) {
      const { hue } = coverArt("album", id, "T", null).paint;
      counts.set(hue, (counts.get(hue) ?? 0) + 1);
    }
    expect(counts.size).toBe(COVER_HUES.length);
    for (const count of counts.values()) expect(count).toBeLessThanOrEqual(150);
  }
});

test("both lightness tiers, every motif and every orientation are used", () => {
  const paints = Array.from(
    { length: 1000 },
    (_, index) => coverArt("album", `release-${index}`, "T", null).paint,
  );
  expect(new Set(paints.map((paint) => paint.tier))).toEqual(new Set([0, 1]));
  expect(new Set(paints.map((paint) => paint.motif)).size).toBe(6);
  expect(new Set(paints.map((paint) => paint.orientation)).size).toBe(4);
  expect(new Set(paints.map((paint) => paint.base)).size).toBe(24);
});

test("distinct ids rarely share the same art", () => {
  const keys = new Set(
    Array.from({ length: 400 }, (_, index) => {
      const { paint } = coverArt("album", `release-${index}`, "T", null);
      return [paint.base, paint.accent, paint.motif, paint.orientation].join();
    }),
  );
  expect(keys.size).toBeGreaterThan(300);
});

test("white text keeps 4.5:1 on the whole text band for every hue and tier", () => {
  const textTop = 0.58;
  for (const kind of kinds) {
    for (let index = 0; index < 600; index += 1) {
      const { paint } = coverArt(kind, `release-${index}`, "T", null);
      const background = mix(paint.base, paint.deep, textTop);
      expect(contrast("#ffffff", background)).toBeGreaterThanOrEqual(4.5);
      const secondary = mix("#ffffff", background, 0.15);
      expect(contrast(secondary, background)).toBeGreaterThanOrEqual(4.5);
      expect(contrast("#ffffff", paint.deep)).toBeGreaterThanOrEqual(4.5);
    }
  }
});

test("the monogram keeps 3:1 over the faded motif in the small tier", () => {
  for (let index = 0; index < 600; index += 1) {
    const { paint } = coverArt("album", `release-${index}`, "T", null);
    const centre = mix(paint.base, paint.deep, 0.5);
    const faded = mix(centre, paint.accent, 0.3);
    expect(contrast("#ffffff", faded)).toBeGreaterThanOrEqual(3);
  }
});

test("accent shapes are never drawn where the text sits", () => {
  for (const kind of kinds) {
    const { layout } = coverArt(kind, "release-1", "T", null);
    expect(layout.motifHeight).toBeLessThanOrEqual(52);
    expect(layout.textTop).toBeGreaterThanOrEqual(layout.motifHeight);
  }
});

test("long titles use a smaller text step", () => {
  const scale = (title: string) =>
    coverArt("album", "r", title, null).text.scale;
  expect(scale("Casa Aberta")).toBe("large");
  expect(scale("Everything I Never Said to You")).toBe("medium");
  expect(
    scale("Everything I Never Said to You While the Tape Was Still Rolling"),
  ).toBe("small");
  expect(scale("夜明けの海は静かに光る")).toBe("medium");
});

test("the monogram is one letter or number, uppercased", () => {
  expect(coverMonogram("casa aberta")).toBe("C");
  expect(coverMonogram("  (Nocturnes)")).toBe("N");
  expect(coverMonogram("1989")).toBe("1");
  expect(coverMonogram("ølen")).toBe("Ø");
  expect(coverMonogram("")).toBe("·");
  expect(coverMonogram("---")).toBe("·");
});

test("the monogram keeps emoji, CJK and combining marks whole", () => {
  expect(coverMonogram("🌙 Nocturnes")).toBe("🌙");
  expect(coverMonogram("👩‍👩‍👧 Família")).toBe("👩‍👩‍👧");
  expect(coverMonogram("🇦🇴 Luanda")).toBe("🇦🇴");
  expect(coverMonogram("夜明けの海")).toBe("夜");
  expect(coverMonogram("étoile")).toBe("É");
  expect(coverMonogram("ßeta")).toBe("ß");
  expect(coverMonogram("́oops")).toBe("O");
});

test("genre art stays in calm hues for the base and the accent, never red or orange", () => {
  const seen = new Set<number>();
  for (let index = 0; index < 400; index += 1) {
    const { paint } = coverArt("genre", `genre-${index}`, "Fado", null);
    seen.add(paint.hue);
    expect(GENRE_HUES).toContain(paint.hue);
    expect(GENRE_HUES).toContain(paint.accentHue);
  }
  expect(seen.size).toBe(GENRE_HUES.length);
  for (const hue of GENRE_HUES) {
    expect(hue).toBeGreaterThanOrEqual(120);
    expect(hue).toBeLessThanOrEqual(300);
  }
});
