import { useCallback, useSyncExternalStore } from "react";

const CHANGED = "hito:account-switch-changed";

function read(name: string, account: string): boolean {
  try {
    return localStorage.getItem(`hito:${name}:${account}`) !== "off";
  } catch {
    return false;
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

export function useAccountSwitch(name: string, account: string) {
  const enabled = useSyncExternalStore(subscribe, () => read(name, account));
  const set = useCallback(
    (value: boolean) => {
      try {
        localStorage.setItem(`hito:${name}:${account}`, value ? "on" : "off");
      } catch {
        return;
      }
      window.dispatchEvent(new Event(CHANGED));
    },
    [name, account],
  );
  return [enabled, set] as const;
}
