import type { PlaybackView } from "./model";

export class BrowserTraversal {
  private order: string[] | null = null;
  private failed = new Set<string>();

  toggle(entries: readonly string[], current?: string) {
    this.order = this.order ? null : this.shuffled(entries, current);
    return this.order !== null;
  }

  reset(entries: readonly string[], current?: string) {
    this.resetFailures();
    if (this.order) this.order = this.shuffled(entries, current);
  }

  remove(entry: string) {
    this.failed.delete(entry);
    if (this.order) this.order = this.order.filter((id) => id !== entry);
  }

  resetFailures() {
    this.failed.clear();
  }

  next(
    entries: readonly string[],
    current: string | undefined,
    repeat: PlaybackView["repeat"],
    reason: "requested" | "completed" | "failed",
  ) {
    if (!current) return null;
    if (reason === "completed" && repeat === "one") return current;
    if (reason === "failed") this.failed.add(current);
    if (reason === "requested") this.resetFailures();
    const order = this.order ?? entries;
    const index = order.indexOf(current);
    for (let next = index + 1; next < order.length; next++)
      if (!this.failed.has(order[next])) return order[next];
    if (repeat === "all")
      for (let next = 0; next <= index; next++)
        if (!this.failed.has(order[next])) return order[next];
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
