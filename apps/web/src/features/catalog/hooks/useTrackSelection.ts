import { useCallback, useMemo, useState } from "react";

export type TrackSelection = {
  selected: ReadonlySet<string>;
  toggle(id: string): void;
  select(ids: string[]): void;
  clear(): void;
};

export function useTrackSelection(available: string[]): TrackSelection {
  const [chosen, setChosen] = useState<ReadonlySet<string>>(new Set());
  const selected = useMemo(
    () => new Set(available.filter((id) => chosen.has(id))),
    [available, chosen],
  );
  const toggle = useCallback((id: string) => {
    setChosen((current) => {
      const next = new Set(current);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  }, []);
  const select = useCallback((ids: string[]) => setChosen(new Set(ids)), []);
  const clear = useCallback(() => setChosen(new Set()), []);
  return { selected, toggle, select, clear };
}
