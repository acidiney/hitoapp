import type { PlaybackView } from "./model";

export class BrowserTraversal {
  private order: string[] | null = null;

  toggle(entries: readonly string[], current?: string) {
    this.order = this.order ? null : this.shuffled(entries, current);
    return this.order !== null;
  }

  reset(entries: readonly string[], current?: string) {
    if (this.order) this.order = this.shuffled(entries, current);
  }

  remove(entry: string) {
    if (this.order) this.order = this.order.filter((id) => id !== entry);
  }

  next(
    entries: readonly string[],
    current: string | undefined,
    repeat: PlaybackView["repeat"],
    completed: boolean,
  ) {
    if (!current) return null;
    if (completed && repeat === "one") return current;
    const order = this.order ?? entries;
    return (
      order[order.indexOf(current) + 1] ?? (repeat === "all" ? order[0] : null)
    );
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
