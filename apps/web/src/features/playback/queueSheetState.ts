import { useSyncExternalStore } from "react";

const listeners = new Set<() => void>();
let open = false;

export function setQueueSheetOpen(next: boolean) {
  if (next === open) return;
  open = next;
  listeners.forEach((listener) => listener());
}

export function toggleQueueSheet() {
  setQueueSheetOpen(!open);
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useQueueSheetOpen(): boolean {
  return useSyncExternalStore(
    subscribe,
    () => open,
    () => false,
  );
}
