import { describe, expect, test } from "vitest";
import {
  acceptedCount,
  indexAfterMove,
  indexAfterRemove,
  isValidPage,
  nextRepeat,
  planMove,
  previousIndex,
  successorAfterRemove,
} from "../browser/QueueModel";

describe("planMove", () => {
  test("moves before a later entry by landing one slot earlier", () => {
    expect(planMove(5, 1, 4)).toBe(3);
  });
  test("moves before an earlier entry onto its slot", () => {
    expect(planMove(5, 3, 1)).toBe(1);
  });
  test("null moves to the end", () => {
    expect(planMove(5, 1, null)).toBe(4);
  });
  test("reports nothing to do for in-place moves", () => {
    expect(planMove(5, 2, 2)).toBeNull();
    expect(planMove(5, 2, 3)).toBeNull();
    expect(planMove(5, 4, null)).toBeNull();
  });
});

describe("indexAfterMove", () => {
  test("the moved current entry follows its destination", () => {
    expect(indexAfterMove(2, 2, 4)).toBe(4);
  });
  test("an entry leaving from before to after the current shifts it up", () => {
    expect(indexAfterMove(2, 0, 3)).toBe(1);
  });
  test("an entry leaving from after to before the current shifts it down", () => {
    expect(indexAfterMove(2, 4, 1)).toBe(3);
  });
  test("moves that do not cross the current leave it", () => {
    expect(indexAfterMove(2, 0, 1)).toBe(2);
    expect(indexAfterMove(2, 3, 4)).toBe(2);
  });
});

describe("removal", () => {
  test("removing before the current shifts it up", () => {
    expect(indexAfterRemove(3, 1)).toBe(2);
  });
  test("removing at or after the current leaves it", () => {
    expect(indexAfterRemove(3, 4)).toBe(3);
  });
  test("a removed current entry is followed by the shifted successor", () => {
    expect(successorAfterRemove(2, 3, 5)).toBe(2);
    expect(successorAfterRemove(2, 1, 5)).toBe(1);
  });
  test("no successor, or the same slot, ends past the queue", () => {
    expect(successorAfterRemove(2, null, 4)).toBe(4);
    expect(successorAfterRemove(2, 2, 4)).toBe(4);
  });
});

describe("acceptedCount", () => {
  test("accepts everything while there is room", () => {
    expect(acceptedCount(10, 5, 100)).toBe(5);
  });
  test("truncates at the cap", () => {
    expect(acceptedCount(98, 5, 100)).toBe(2);
  });
  test("accepts nothing past the cap", () => {
    expect(acceptedCount(100, 5, 100)).toBe(0);
    expect(acceptedCount(120, 5, 100)).toBe(0);
  });
});

describe("isValidPage", () => {
  test("accepts offsets up to the length and limits 1 to 1000", () => {
    expect(isValidPage(0, 1, 0)).toBe(true);
    expect(isValidPage(5, 1000, 5)).toBe(true);
  });
  test("rejects bad offsets and limits", () => {
    expect(isValidPage(-1, 10, 5)).toBe(false);
    expect(isValidPage(6, 10, 5)).toBe(false);
    expect(isValidPage(1.5, 10, 5)).toBe(false);
    expect(isValidPage(0, 0, 5)).toBe(false);
    expect(isValidPage(0, 1001, 5)).toBe(false);
    expect(isValidPage(0, 1.5, 5)).toBe(false);
  });
});

test("repeat cycles off, all, one", () => {
  expect(nextRepeat("off")).toBe("all");
  expect(nextRepeat("all")).toBe("one");
  expect(nextRepeat("one")).toBe("off");
});

test("previous never goes below the first entry", () => {
  expect(previousIndex(3)).toBe(2);
  expect(previousIndex(0)).toBe(0);
  expect(previousIndex(-1)).toBe(0);
});
