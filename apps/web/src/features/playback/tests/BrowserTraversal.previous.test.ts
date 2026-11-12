import { expect, test } from "vitest";
import { BrowserTraversal } from "../browser/BrowserTraversal";

test("previous with shuffle on returns the entry that played before, not the one before it in the list", () => {
  for (let trial = 0; trial < 40; trial++) {
    const traversal = new BrowserTraversal();
    const entries = [10, 11, 12, 13, 14, 15];
    traversal.toggle(entries, 10);
    const played = [0];
    for (let step = 0; step < 4; step++) {
      const next = traversal.next(
        entries,
        played[played.length - 1],
        "off",
        "requested",
      );
      if (next === null) break;
      played.push(next);
    }
    for (let back = played.length - 1; back > 0; back--)
      expect(traversal.previous(entries, played[back])).toBe(played[back - 1]);
    expect(traversal.previous(entries, played[0])).toBe(played[0]);
  }
});

test("previous with shuffle off is the entry before in list order and stays at the first", () => {
  const traversal = new BrowserTraversal();
  const entries = [10, 11, 12];
  expect(traversal.previous(entries, 2)).toBe(1);
  expect(traversal.previous(entries, 0)).toBe(0);
});
