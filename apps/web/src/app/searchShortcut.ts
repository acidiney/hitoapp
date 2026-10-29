import { router } from "@inertiajs/react";

export const SHELL_SEARCH_ID = "shell-search";

export function isApplePlatform(
  platform: string = typeof navigator === "undefined" ? "" : navigator.platform,
): boolean {
  return /Mac|iPhone|iPad|iPod/.test(platform);
}

export function shortcutLabel(apple: boolean): { text: string; keys: string } {
  return apple
    ? { text: "⌘K", keys: "Meta+K" }
    : { text: "Ctrl K", keys: "Control+K" };
}

export function focusShellSearch(): boolean {
  const field = document.getElementById(SHELL_SEARCH_ID);
  if (!(field instanceof HTMLInputElement)) return false;
  field.focus();
  field.select();
  return true;
}

function focusOnArrival(attempts = 20) {
  if (focusShellSearch() || attempts <= 0) return;
  setTimeout(() => focusOnArrival(attempts - 1), 25);
}

export function openSearch() {
  if (focusShellSearch()) return;
  router.get("/library", {}, { onSuccess: () => focusOnArrival() });
}

export function handleSearchShortcut(event: KeyboardEvent, apple: boolean) {
  if (
    event.defaultPrevented ||
    event.isComposing ||
    event.shiftKey ||
    event.altKey ||
    event.key.toLowerCase() !== "k"
  )
    return;
  const primary = apple
    ? event.metaKey && !event.ctrlKey
    : event.ctrlKey && !event.metaKey;
  if (!primary) return;
  if (document.querySelector("dialog[open], [aria-modal='true']")) return;
  event.preventDefault();
  openSearch();
}
