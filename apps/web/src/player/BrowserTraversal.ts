import type { PlaybackView } from "./model";

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
