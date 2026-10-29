import { createContext, useContext, useEffect, useId } from "react";

export type UnsavedRegistry = {
  report(id: string, dirty: boolean): void;
  hasUnsaved(): boolean;
};

export function createUnsavedRegistry(): UnsavedRegistry {
  const dirty = new Set<string>();
  return {
    report(id, value) {
      if (value) dirty.add(id);
      else dirty.delete(id);
    },
    hasUnsaved: () => dirty.size > 0,
  };
}

export const UnsavedContext = createContext<UnsavedRegistry | null>(null);

export function useReportUnsaved(dirty: boolean) {
  const registry = useContext(UnsavedContext);
  const id = useId();
  useEffect(() => {
    registry?.report(id, dirty);
    return () => registry?.report(id, false);
  }, [registry, id, dirty]);
}
