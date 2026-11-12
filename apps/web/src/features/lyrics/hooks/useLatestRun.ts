import { useMemo } from "react";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";

export type Run = {
  signal: AbortSignal;
  active: () => boolean;
  finish: () => void;
};

type Owner = {
  scope: ReturnType<typeof useRequestScope>;
  generation: number;
  controller: AbortController | null;
  pending: boolean;
};

export function useLatestRun(scopeKey: string) {
  const scope = useRequestScope(scopeKey);
  const owner = useMemo<Owner>(
    () => ({ scope, generation: 0, controller: null, pending: false }),
    [scope],
  );

  function cancel() {
    if (!owner.scope.capture().active()) return;
    owner.generation++;
    owner.controller?.abort();
    owner.controller = null;
    owner.pending = false;
  }

  function start(deadlineMs: number, onDeadline: () => void): Run | null {
    const request = owner.scope.capture();
    if (!request.active() || owner.pending) return null;
    const generation = ++owner.generation;
    const controller = new AbortController();
    owner.controller = controller;
    owner.pending = true;
    const signal = AbortSignal.any([request.signal, controller.signal]);
    const active = () =>
      request.active() && owner.generation === generation && !signal.aborted;
    const timer = setTimeout(() => {
      if (!active()) return;
      controller.abort();
      owner.pending = false;
      onDeadline();
    }, deadlineMs);
    return {
      signal,
      active,
      finish: () => {
        clearTimeout(timer);
        if (owner.controller === controller) {
          owner.controller = null;
          owner.pending = false;
        }
      },
    };
  }

  return { owner, start, cancel };
}
