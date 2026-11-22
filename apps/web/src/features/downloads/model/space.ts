export const GIB = 1024 ** 3;
const MIB = 1024 ** 2;
export const LARGEST_BUDGET_GIB = 1024;

export function spaceText(bytes: number): string {
  if (bytes < GIB) return `${Math.round(bytes / MIB)} MiB`;
  return `${Number((bytes / GIB).toFixed(1))} GiB`;
}

export function budgetGib(bytes: number): string {
  return String(Number((bytes / GIB).toFixed(2)));
}

export function budgetBytes(text: string): number | null {
  const trimmed = text.trim();
  if (!/^\d+(\.\d+)?$/.test(trimmed)) return null;
  const value = Number(trimmed);
  if (!Number.isFinite(value) || value < 0 || value > LARGEST_BUDGET_GIB)
    return null;
  return Math.round(value * GIB);
}
