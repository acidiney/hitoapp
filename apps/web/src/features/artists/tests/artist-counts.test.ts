import { expect, test } from "vitest";
import { artistCounts } from "../model/artistCounts";

test("tracks and albums use one format with correct singular and plural", () => {
  expect(artistCounts(4, 2)).toBe("4 tracks · 2 albums");
  expect(artistCounts(1, 1)).toBe("1 track · 1 album");
});

test("a zero part is left out and an empty credit still says something", () => {
  expect(artistCounts(0, 1)).toBe("1 album");
  expect(artistCounts(3, 0)).toBe("3 tracks");
  expect(artistCounts(0, 0)).toBe("0 tracks");
});
