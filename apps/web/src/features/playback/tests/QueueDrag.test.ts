import { expect, test } from "vitest";
import {
  clampTravel,
  dropSlot,
  edgeSpeed,
  insertionIndex,
  shiftFor,
} from "../hooks/queueDrag";

const ROW = 60;

test("the drop slot is the nearest row to the dragged row and stays inside the list", () => {
  expect(dropSlot(2, 0, 0, 10, ROW)).toBe(2);
  expect(dropSlot(2, 2 * ROW + 20, 0, 10, ROW)).toBe(4);
  expect(dropSlot(2, -2 * ROW - 20, 0, 10, ROW)).toBe(0);
  expect(dropSlot(2, 40 * ROW, 0, 10, ROW)).toBe(9);
  expect(dropSlot(5, -40 * ROW, 4, 10, ROW)).toBe(4);
});

test("travel cannot leave the list", () => {
  expect(clampTravel(-1000, 2, 0, 10, ROW)).toBe(-2 * ROW);
  expect(clampTravel(1000, 2, 0, 10, ROW)).toBe(7 * ROW);
  expect(clampTravel(30, 2, 0, 10, ROW)).toBe(30);
});

test("neighbours step aside by one row between the start and the slot", () => {
  const shifts = (from: number, over: number) =>
    [0, 1, 2, 3, 4, 5].map((index) => shiftFor(index, from, over, ROW));
  expect(shifts(1, 3)).toEqual([0, 0, -ROW, -ROW, 0, 0]);
  expect(shifts(4, 2)).toEqual([0, 0, ROW, ROW, 0, 0]);
  expect(shifts(3, 3)).toEqual([0, 0, 0, 0, 0, 0]);
});

test("the insertion point is before the row that follows the slot", () => {
  expect(insertionIndex(1, 3)).toBe(4);
  expect(insertionIndex(4, 2)).toBe(2);
});

test("the list scrolls only near its edges and faster closer to them", () => {
  expect(edgeSpeed(300, 100, 500)).toBe(0);
  expect(edgeSpeed(110, 100, 500)).toBeLessThan(0);
  expect(edgeSpeed(490, 100, 500)).toBeGreaterThan(0);
  expect(Math.abs(edgeSpeed(100, 100, 500))).toBeGreaterThan(
    Math.abs(edgeSpeed(140, 100, 500)),
  );
});
