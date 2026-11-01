import type { Schema } from "../../../shared/schema";

export const MIN_CHECK_MS = 5_000;

export function msUntilCheck(
  status: Pick<Schema<"SourcePreparationStatus">, "next_check_at"> | null,
  now = Date.now(),
) {
  const due = status?.next_check_at;
  return due == null ? 0 : Math.max(0, due * 1000 - now);
}
