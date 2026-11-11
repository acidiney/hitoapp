import type { Schema } from "../../../shared/schema";

type Line = Pick<Schema<"TimedLyricLine">, "start_ms" | "end_ms">;

export function isSynced(lines: Line[]): boolean {
  if (lines.length < 2) return false;
  let ordered = true;
  for (let index = 1; index < lines.length; index += 1)
    if (lines[index].start_ms < lines[index - 1].start_ms) ordered = false;
  const first = lines[0].start_ms;
  return ordered && lines.some((line) => line.start_ms > first);
}

export function lineAt(lines: Line[], positionMs: number): number {
  let current = -1;
  for (let index = 0; index < lines.length; index += 1) {
    if (lines[index].start_ms > positionMs) break;
    current = index;
  }
  return current;
}
