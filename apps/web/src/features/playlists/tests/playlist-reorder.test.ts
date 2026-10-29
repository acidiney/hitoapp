import { expect, test } from "vitest";
import { reorderMoves } from "../model/reorder";

const entries = (positions: number[]) =>
  positions.map((position, index) => ({ entry_id: `e${index}`, position }));

test("moving an entry down swaps its position with its neighbour", () => {
  expect(reorderMoves(entries([0, 1, 2]), "e0", 1)).toEqual([
    { entry_id: "e1", position: 0 },
    { entry_id: "e0", position: 1 },
  ]);
});

test("moving an entry up swaps with the one above", () => {
  expect(reorderMoves(entries([10, 20, 30]), "e2", -1)).toEqual([
    { entry_id: "e2", position: 20 },
    { entry_id: "e1", position: 30 },
  ]);
});

test("moving past either end changes nothing", () => {
  expect(reorderMoves(entries([0, 1]), "e0", -1)).toEqual([]);
  expect(reorderMoves(entries([0, 1]), "e1", 1)).toEqual([]);
  expect(reorderMoves(entries([0, 1]), "missing", 1)).toEqual([]);
});

test("tied positions are renumbered only around the moved entry", () => {
  expect(reorderMoves(entries([5, 5, 5]), "e0", 1)).toEqual([
    { entry_id: "e0", position: 6 },
    { entry_id: "e2", position: 7 },
  ]);
});

const large = (positions: (index: number) => number) =>
  Array.from({ length: 5000 }, (_, index) => ({
    entry_id: `e${index}`,
    position: positions(index),
  }));

test("one move in a 5,000 entry playlist changes at most two positions", () => {
  const moves = reorderMoves(
    large((index) => index),
    "e2500",
    1,
  )!;
  expect(moves).toEqual([
    { entry_id: "e2501", position: 2500 },
    { entry_id: "e2500", position: 2501 },
  ]);
});

test("a tie in a spaced playlist stays local", () => {
  const list = large((index) => index * 10);
  list[2501] = { ...list[2501], position: list[2500].position };
  const moves = reorderMoves(list, "e2500", 1)!;
  expect(moves.length).toBeLessThanOrEqual(3);
  expect(moves.every((move) => /^e25\d\d$/.test(move.entry_id))).toBe(true);
});

test("a tie that would shift thousands of entries is refused", () => {
  const list = large((index) => (index <= 2500 ? index : index - 1));
  expect(reorderMoves(list, "e2500", 1)).toBeNull();
});
