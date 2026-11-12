import { useRef, type KeyboardEvent } from "react";

export const RESULT_ROW = "data-search-row";

export function useResultKeys() {
  const results = useRef<HTMLDivElement>(null);
  function rows(): HTMLElement[] {
    return [
      ...(results.current?.querySelectorAll<HTMLElement>(`[${RESULT_ROW}]`) ??
        []),
    ];
  }
  function move(event: KeyboardEvent) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const all = rows();
    if (all.length === 0) return;
    const active = document.activeElement;
    const current = all.findIndex((row) => row.contains(active));
    const next =
      event.key === "ArrowDown"
        ? Math.min(current + 1, all.length - 1)
        : current <= 0
          ? -1
          : current - 1;
    event.preventDefault();
    if (next === -1) {
      document.getElementById("shell-search")?.focus();
      return;
    }
    all[next].focus();
  }
  return { results, move };
}
