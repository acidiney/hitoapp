import type { PlaybackView } from "../model";

export class BrowserTraversal {
  private order: string[] | null = null;
  private failed = new Set<string>();
  private canonicalPositions = new Map<string, number>();
  private shufflePositions = new Map<string, number>();

  toggle(entries: readonly string[], current?: string) {
    this.order = this.order ? null : this.shuffled(entries, current);
    this.reindexShuffle();
    return this.order !== null;
  }

  reset(entries: readonly string[], current?: string) {
    this.resetFailures();
    this.canonicalPositions.clear();
    entries.forEach((id, index) => this.canonicalPositions.set(id, index));
    if (this.order) this.order = this.shuffled(entries, current);
    this.reindexShuffle();
  }

  append(added: readonly string[], start: number, current?: string) {
    added.forEach((id, index) =>
      this.canonicalPositions.set(id, start + index),
    );
    if (!this.order) return;
    const at = current ? (this.shufflePositions.get(current) ?? -1) : -1;
    const tail = this.order.slice(at + 1);
    const merged = interleave(tail, this.shuffled(added));
    this.order.length = at + 1;
    for (const id of merged) this.order.push(id);
    for (let index = at + 1; index < this.order.length; index++)
      this.shufflePositions.set(this.order[index], index);
  }

  moved(entries: readonly string[], from: number, to: number) {
    const low = Math.min(from, to);
    const high = Math.max(from, to);
    for (let index = low; index <= high; index++)
      this.canonicalPositions.set(entries[index], index);
  }

  remove(entry: string, entries: readonly string[]) {
    this.failed.delete(entry);
    if (this.order) this.order = this.order.filter((id) => id !== entry);
    this.canonicalPositions.clear();
    entries.forEach((id, index) => this.canonicalPositions.set(id, index));
    this.reindexShuffle();
  }

  private reindexShuffle() {
    this.shufflePositions.clear();
    this.order?.forEach((id, index) => this.shufflePositions.set(id, index));
  }

  resetFailures() {
    this.failed.clear();
  }

  next(
    entries: readonly string[],
    current: number,
    repeat: PlaybackView["repeat"],
    reason: "requested" | "completed" | "failed",
  ) {
    const entry = entries[current];
    if (!entry) return null;
    if (reason === "failed") this.failed.add(entry);
    if (reason === "requested") this.resetFailures();
    return this.successor(entries, current, repeat, reason === "completed");
  }

  completed(
    entries: readonly string[],
    current: number,
    repeat: PlaybackView["repeat"],
  ) {
    return this.successor(entries, current, repeat, true);
  }

  private successor(
    entries: readonly string[],
    current: number,
    repeat: PlaybackView["repeat"],
    completed: boolean,
  ) {
    const entry = entries[current];
    if (!entry) return null;
    if (completed && repeat === "one") return current;
    const order = this.order ?? entries;
    const index = this.order ? this.shufflePositions.get(entry) : current;
    if (index === undefined) return null;
    for (let next = index + 1; next < order.length; next++)
      if (!this.failed.has(order[next]))
        return this.canonicalPositions.get(order[next]) ?? null;
    if (repeat === "all")
      for (let next = 0; next <= index; next++)
        if (!this.failed.has(order[next]))
          return this.canonicalPositions.get(order[next]) ?? null;
    return null;
  }

  private shuffled(entries: readonly string[], current?: string) {
    const order = entries.filter((id) => id !== current);
    for (let index = order.length - 1; index > 0; index--) {
      const other = Math.floor(Math.random() * (index + 1));
      [order[index], order[other]] = [order[other], order[index]];
    }
    if (current) order.unshift(current);
    return order;
  }
}

function interleave(first: string[], second: string[]) {
  const merged: string[] = [];
  let a = 0;
  let b = 0;
  while (a < first.length || b < second.length) {
    const left = first.length - a;
    const right = second.length - b;
    merged.push(
      right === 0 || (left > 0 && Math.random() * (left + right) < left)
        ? first[a++]
        : second[b++],
    );
  }
  return merged;
}
