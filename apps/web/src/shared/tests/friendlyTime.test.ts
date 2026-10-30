import { expect, test } from "vitest";
import { friendlyTime } from "../format";

const now = new Date(2026, 9, 29, 15, 0, 0);

test("a time on the same day is labelled Today", () => {
  expect(friendlyTime(new Date(2026, 9, 29, 9, 52), now, "en-GB")).toBe(
    "Today, 09:52",
  );
});

test("the next and previous days are labelled by name", () => {
  expect(friendlyTime(new Date(2026, 9, 30, 9, 52), now, "en-GB")).toBe(
    "Tomorrow, 09:52",
  );
  expect(friendlyTime(new Date(2026, 9, 28, 23, 5), now, "en-GB")).toBe(
    "Yesterday, 23:05",
  );
});

test("later days use a short date with the time", () => {
  const text = friendlyTime(new Date(2026, 10, 3, 9, 52), now, "en-GB");
  expect(text).toContain("09:52");
  expect(text).not.toMatch(/Today|Tomorrow|Yesterday/);
  expect(text).toMatch(/3/);
});

test("midnight rollover compares calendar days, not 24 hour spans", () => {
  const lateNow = new Date(2026, 9, 29, 23, 59);
  expect(friendlyTime(new Date(2026, 9, 30, 0, 1), lateNow, "en-GB")).toBe(
    "Tomorrow, 00:01",
  );
});
