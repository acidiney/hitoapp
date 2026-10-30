import type { PlaybackView } from "../model";

export function planMove(
  length: number,
  from: number,
  before: number | null,
): number | null {
  const to = before === null ? length - 1 : before > from ? before - 1 : before;
  return to === from ? null : to;
}

export function indexAfterMove(current: number, from: number, to: number) {
  if (from === current) return to;
  if (from < current && to >= current) return current - 1;
  if (from > current && to <= current) return current + 1;
  return current;
}

export function indexAfterRemove(current: number, removed: number) {
  return removed < current ? current - 1 : current;
}

export function successorAfterRemove(
  removed: number,
  next: number | null,
  lengthAfter: number,
) {
  if (next === null || next === removed) return lengthAfter;
  return next > removed ? next - 1 : next;
}

export function acceptedCount(length: number, incoming: number, max: number) {
  return Math.min(incoming, Math.max(0, max - length));
}

export function isValidPage(offset: number, limit: number, length: number) {
  return (
    Number.isInteger(offset) &&
    offset >= 0 &&
    offset <= length &&
    Number.isInteger(limit) &&
    limit >= 1 &&
    limit <= 1000
  );
}

export function nextRepeat(
  mode: PlaybackView["repeat"],
): PlaybackView["repeat"] {
  return mode === "off" ? "all" : mode === "all" ? "one" : "off";
}

export function previousIndex(current: number) {
  return Math.max(0, current - 1);
}
