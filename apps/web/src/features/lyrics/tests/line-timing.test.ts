import { expect, test } from "vitest";
import { isSynced, lineAt } from "../model/lineTiming";

const line = (start_ms: number, end_ms = start_ms + 1000) => ({
  start_ms,
  end_ms,
});

test("lines with rising start times are synced", () => {
  expect(isSynced([line(0), line(2000), line(4000)])).toBe(true);
});

test("a single line, equal start times or out of order times are not synced", () => {
  expect(isSynced([line(0)])).toBe(false);
  expect(isSynced([line(0, 1), line(0, 1)])).toBe(false);
  expect(isSynced([line(0), line(4000), line(2000)])).toBe(false);
  expect(isSynced([])).toBe(false);
});

test("the current line is the last one that has started", () => {
  const lines = [line(1000), line(3000), line(6000)];
  expect(lineAt(lines, 0)).toBe(-1);
  expect(lineAt(lines, 1000)).toBe(0);
  expect(lineAt(lines, 5999)).toBe(1);
  expect(lineAt(lines, 60000)).toBe(2);
});
