import { useEffect, useMemo, useRef } from "react";

export function useRequestScope(key: string) {
  const owner = useMemo(() => ({ controller: new AbortController() }), [key]);
  const latest = useRef(owner);
  latest.current = owner;
  useEffect(() => {
    if (owner.controller.signal.aborted)
      owner.controller = new AbortController();
    return () => owner.controller.abort();
  }, [owner]);
  return useMemo(
    () => ({
      capture: () => {
        const controller = owner.controller;
        return {
          signal: controller.signal,
          active: () =>
            latest.current === owner &&
            owner.controller === controller &&
            !controller.signal.aborted,
        };
      },
    }),
    [owner],
  );
}
