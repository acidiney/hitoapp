import { afterEach, expect, test } from "vitest";
import { keyboardInset, lockPageScroll } from "./sheetViewport";

afterEach(() => document.documentElement.removeAttribute("data-dialog-open"));

test("the keyboard inset is the part of the layout viewport the visual viewport no longer covers", () => {
  expect(keyboardInset({ innerHeight: 844, height: 844, offsetTop: 0 })).toBe(
    0,
  );
  expect(keyboardInset({ innerHeight: 844, height: 544, offsetTop: 0 })).toBe(
    300,
  );
  expect(keyboardInset({ innerHeight: 844, height: 544, offsetTop: 120 })).toBe(
    180,
  );
  expect(keyboardInset({ innerHeight: 600, height: 700, offsetTop: 0 })).toBe(
    0,
  );
});

test("page scroll stays locked until the last dialog unlocks it", () => {
  const first = lockPageScroll();
  const second = lockPageScroll();
  first();
  expect(document.documentElement.hasAttribute("data-dialog-open")).toBe(true);
  second();
  expect(document.documentElement.hasAttribute("data-dialog-open")).toBe(false);
});
