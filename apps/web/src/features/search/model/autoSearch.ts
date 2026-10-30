import { useCallback, useSyncExternalStore } from "react";

const CHANGED = "hito:search-services-auto-changed";
const key = (account: string) => `hito:search-services-auto:${account}`;

function read(account: string): boolean {
  try {
    return localStorage.getItem(key(account)) !== "off";
  } catch {
    return true;
  }
}

function subscribe(notify: () => void) {
  window.addEventListener(CHANGED, notify);
  window.addEventListener("storage", notify);
  return () => {
    window.removeEventListener(CHANGED, notify);
    window.removeEventListener("storage", notify);
  };
}

/** Whether a search with few library results is also sent to the connected services. */
export function useAutomaticServiceSearch(account: string) {
  const enabled = useSyncExternalStore(subscribe, () => read(account));
  const set = useCallback(
    (value: boolean) => {
      try {
        localStorage.setItem(key(account), value ? "on" : "off");
      } catch {
        return;
      }
      window.dispatchEvent(new Event(CHANGED));
    },
    [account],
  );
  return [enabled, set] as const;
}
