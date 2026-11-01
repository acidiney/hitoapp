import { expect, test } from "vitest";
import { regionName } from "../regionName";

test("a country code becomes the English country name", () => {
  expect(regionName("GB")).toBe("United Kingdom");
  expect(regionName("PT")).toBe("Portugal");
});

test("a lowercase code is read the same way", () => {
  expect(regionName("us")).toBe("United States");
});

test("a code that is not a region keeps its raw text", () => {
  expect(regionName("XX1")).toBe("XX1");
  expect(regionName("")).toBe("");
});
