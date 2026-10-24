import type { PlaybackView } from "../model";

export class BrowserTraversal {
  private order: number[] | null = null;
  private failed = new Set<number>();

  toggle(entries: readonly number[], current?: number) {
    this.order = this.order ? null : this.shuffled(entries, current);
    return this.order !== null;
  }

  reset(entries: readonly number[], current?: number) {
    this.resetFailures();
    if (this.order) this.order = this.shuffled(entries, current);
  }

  append(added: readonly number[], current?: number) {
    const order = this.order;
    if (!order) return;
    const at = current === undefined ? -1 : order.indexOf(current);
    const from = at < 0 ? order.length : at + 2;
    const tail = order.splice(from);
    const mixed = interleave(tail, this.shuffled(added));
    for (const id of mixed) order.push(id);
  }

  remove(entry: number) {
    this.failed.delete(entry);
    if (!this.order) return;
    const index = this.order.indexOf(entry);
    if (index >= 0) this.order.splice(index, 1);
  }

  resetFailures() {
    this.failed.clear();
  }

  next(
    entries: readonly number[],
    current: number,
    repeat: PlaybackView["repeat"],
    reason: "requested" | "completed" | "failed",
  ) {
    const entry = entries[current];
    if (entry === undefined) return null;
    if (reason === "failed") this.failed.add(entry);
    if (reason === "requested") this.resetFailures();
    return this.successor(entries, current, repeat, reason === "completed");
  }

  completed(
    entries: readonly number[],
    current: number,
    repeat: PlaybackView["repeat"],
  ) {
    return this.successor(entries, current, repeat, true);
  }

  private successor(
    entries: readonly number[],
    current: number,
    repeat: PlaybackView["repeat"],
    completed: boolean,
  ) {
    const entry = entries[current];
    if (entry === undefined) return null;
    if (completed && repeat === "one") return current;
    const order = this.order ?? entries;
    const index = this.order ? this.order.indexOf(entry) : current;
    if (index < 0) return null;
    const found = (from: number, to: number) => {
      for (let next = from; next < to; next++)
        if (!this.failed.has(order[next]))
          return this.order ? entries.indexOf(order[next]) : next;
      return null;
    };
    const following = found(index + 1, order.length);
    if (following !== null) return following;
    return repeat === "all" ? found(0, index + 1) : null;
  }

  private shuffled(entries: readonly number[], current?: number) {
    const order = entries.filter((id) => id !== current);
    for (let index = order.length - 1; index > 0; index--) {
      const other = Math.floor(Math.random() * (index + 1));
      [order[index], order[other]] = [order[other], order[index]];
    }
    if (current !== undefined) order.unshift(current);
    return order;
  }
}

function interleave(first: number[], second: number[]) {
  const merged: number[] = [];
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
