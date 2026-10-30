import { expect, test } from "vitest";
import { COVER_PALETTE, coverStyle } from "../ui/coverPalette";
import { headerCoverId, tintFor } from "../ui/collectionTint";

test("a collection keeps the same flat colour for the same release id", () => {
  expect(tintFor("release-0")).toBe(tintFor("release-0"));
});

test("tints come from the cover palette so white text stays readable", () => {
  const ids = Array.from({ length: 100 }, (_, index) => `release-${index}`);
  for (const id of ids)
    expect(COVER_PALETTE.map((entry) => entry.background)).toContain(
      tintFor(id),
    );
});

test("a known id keeps its documented tint", () => {
  expect(tintFor("release-0")).toBe("#b5412b");
});

test("a generated header cover never shares the header colour", () => {
  for (let index = 0; index < 100; index += 1) {
    const id = `playlist-${index}`;
    expect(coverStyle(headerCoverId(id)).background).not.toBe(tintFor(id));
    expect(headerCoverId(id)).toBe(headerCoverId(id));
  }
});
