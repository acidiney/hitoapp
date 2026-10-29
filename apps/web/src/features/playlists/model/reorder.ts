export type OrderedEntry = { entry_id: string; position: number };
export type EntryMove = { entry_id: string; position: number };

/**
 * The position changes that move one entry a single step. Entries must be in
 * display order. Distinct positions are swapped; tied positions are renumbered
 * so the stored order matches what is shown.
 */
export function reorderMoves(
  entries: OrderedEntry[],
  entryId: string,
  delta: -1 | 1,
): EntryMove[] {
  const from = entries.findIndex((entry) => entry.entry_id === entryId);
  const to = from + delta;
  if (from < 0 || to < 0 || to >= entries.length) return [];
  const order = entries.map((entry) => entry.entry_id);
  [order[from], order[to]] = [order[to], order[from]];
  const positions = entries.map((entry) => entry.position);
  const distinct = new Set(positions).size === positions.length;
  const next = distinct ? positions : positions.map((_, index) => index);
  const current = new Map(
    entries.map((entry) => [entry.entry_id, entry.position]),
  );
  return order
    .map((id, index) => ({ entry_id: id, position: next[index] }))
    .filter((move) => current.get(move.entry_id) !== move.position);
}
