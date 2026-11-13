import { expect, test } from "vitest";
import { albumEyebrow } from "../model/albumEyebrow";

test("a library album without a kind reads Album and its year", () => {
  expect(albumEyebrow(null, 2025)).toBe("Album · 2025");
  expect(albumEyebrow(undefined, null)).toBe("Album");
});

test("a catalog release shows its kind and year", () => {
  expect(albumEyebrow("ep", "2019")).toBe("EP · 2019");
  expect(albumEyebrow("single", undefined)).toBe("Single");
  expect(albumEyebrow("album", "2001")).toBe("Album · 2001");
});
