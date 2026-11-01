export type OrderedEntry = { entry_id: string; position: number };
export type EntryMove = { entry_id: string; position: number };

export const MAX_REORDER_MOVES = 200;

export function reorderMoves(
  entries: OrderedEntry[],
  entryId: string,
  delta: -1 | 1,
): EntryMove[] | null {
  const from = entries.findIndex((entry) => entry.entry_id === entryId);
  const to = from + delta;
  if (from < 0 || to < 0 || to >= entries.length) return [];
  const order = entries.map((entry) => entry.entry_id);
  [order[from], order[to]] = [order[to], order[from]];
  let low = Math.min(from, to);
  let high = Math.max(from, to);
  while (low > 0 && entries[low - 1].position === entries[low].position)
    low -= 1;
  while (
    high < entries.length - 1 &&
    entries[high + 1].position === entries[high].position
  )
    high += 1;
  const slots = new Set<number>();
  for (let index = low; index <= high; index += 1)
    slots.add(entries[index].position);
  const next = new Map<string, number>();
  if (slots.size === high - low + 1) {
    for (let index = low; index <= high; index += 1)
      next.set(order[index], entries[index].position);
  } else {
    let position = entries[low].position;
    for (let index = low; index <= high; index += 1, position += 1)
      next.set(order[index], position);
    for (let index = high + 1; index < entries.length; index += 1) {
      if (entries[index].position >= position) break;
      next.set(order[index], position);
      position += 1;
      if (next.size > MAX_REORDER_MOVES) return null;
    }
  }
  const current = new Map(
    entries.map((entry) => [entry.entry_id, entry.position]),
  );
  const moves = [...next]
    .map(([id, position]) => ({ entry_id: id, position }))
    .filter((move) => current.get(move.entry_id) !== move.position);
  return moves.length > MAX_REORDER_MOVES ? null : moves;
}
