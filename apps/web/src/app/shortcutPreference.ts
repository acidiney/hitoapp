import { useCallback, useState } from "react";

const KEY = "hito.shortcuts.single";

function read(): boolean {
  try {
    return window.localStorage.getItem(KEY) !== "off";
  } catch {
    return true;
  }
}

export function useSingleKeyShortcuts(): [boolean, (enabled: boolean) => void] {
  const [enabled, setEnabled] = useState(read);
  const change = useCallback((next: boolean) => {
    setEnabled(next);
    try {
      window.localStorage.setItem(KEY, next ? "on" : "off");
    } catch {
      return;
    }
  }, []);
  return [enabled, change];
}
