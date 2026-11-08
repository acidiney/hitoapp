import { useSyncExternalStore } from "react";

export const PHONE_QUERY = "(max-width: 767px)";

function subscribe(query: string, notify: () => void): () => void {
  if (typeof window.matchMedia !== "function") return () => {};
  const list = window.matchMedia(query);
  list.addEventListener("change", notify);
  return () => list.removeEventListener("change", notify);
}

function matches(query: string): boolean {
  return (
    typeof window.matchMedia === "function" && window.matchMedia(query).matches
  );
}

export function useMediaQuery(query: string): boolean {
  return useSyncExternalStore(
    (notify) => subscribe(query, notify),
    () => matches(query),
    () => false,
  );
}
