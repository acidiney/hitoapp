import { useSyncExternalStore } from "react";

const STORAGE_KEY = "hito.queue-panel";
const listeners = new Set<() => void>();

function stored(): boolean {
  try {
    return window.localStorage.getItem(STORAGE_KEY) === "open";
  } catch {
    return false;
  }
}

let open = typeof window === "undefined" ? false : stored();

export function setQueuePanelOpen(next: boolean) {
  if (next === open) return;
  open = next;
  try {
    window.localStorage.setItem(STORAGE_KEY, next ? "open" : "closed");
  } catch {
    void 0;
  }
  listeners.forEach((listener) => listener());
}

export function toggleQueuePanel() {
  setQueuePanelOpen(!open);
}

export function resetQueuePanelForTests() {
  open = stored();
  listeners.forEach((listener) => listener());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useQueuePanelOpen(): boolean {
  return useSyncExternalStore(
    subscribe,
    () => open,
    () => false,
  );
}

export const QUEUE_PANEL_ID = "queue-panel";
