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

test("tied positions are renumbered so the new order is exact", () => {
  expect(reorderMoves(entries([5, 5, 5]), "e0", 1)).toEqual([
    { entry_id: "e1", position: 0 },
    { entry_id: "e0", position: 1 },
    { entry_id: "e2", position: 2 },
  ]);
});
