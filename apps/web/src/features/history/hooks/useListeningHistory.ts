import { useCallback, useEffect, useState } from "react";
import { errorMessage } from "../../../shared/format";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import { historyApi } from "../api/historyApi";

type Phase = "load" | "save" | "delete" | null;
type State = {
  enabled: boolean | null;
  phase: Phase;
  error: string;
  removed: number | null;
};

const initial: State = {
  enabled: null,
  phase: "load",
  error: "",
  removed: null,
};

export function useListeningHistory(account: string) {
  const scope = useRequestScope(account);
  const [state, setState] = useState<State>(initial);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    const captured = scope.capture();
    const controller = new AbortController();
    const signal = AbortSignal.any([captured.signal, controller.signal]);
    setState(initial);
    historyApi
      .preference(account, signal)
      .then((preference) => {
        if (captured.active() && !signal.aborted)
          setState({
            enabled: preference.enabled,
            phase: null,
            error: "",
            removed: null,
          });
      })
      .catch((error: unknown) => {
        if (captured.active() && !signal.aborted)
          setState({
            enabled: null,
            phase: null,
            error: errorMessage(error),
            removed: null,
          });
      });
    return () => controller.abort();
  }, [account, scope, attempt]);

  const setEnabled = useCallback(
    async (enabled: boolean) => {
      const request = scope.capture();
      const requestId = createUuid();
      const started = performance.now();
      let outcome = "cancelled";
      setState((value) => ({
        ...value,
        phase: "save",
        error: "",
        removed: null,
      }));
      try {
        const preference = await historyApi.setEnabled(
          account,
          enabled,
          request.signal,
        );
        if (!request.active()) return;
        outcome = "accepted";
        setState({
          enabled: preference.enabled,
          phase: null,
          error: "",
          removed: null,
        });
      } catch (error) {
        if (!request.active()) return;
        outcome = "failed";
        setState((value) => ({
          ...value,
          phase: null,
          error: errorMessage(error),
        }));
      } finally {
        log.info("history_preference.finished", {
          request_id: requestId,
          reason: outcome,
          enabled,
          duration_ms: Math.round(performance.now() - started),
        });
      }
    },
    [account, scope],
  );

  const clear = useCallback(async (): Promise<boolean> => {
    const request = scope.capture();
    const requestId = createUuid();
    const started = performance.now();
    let outcome = "cancelled";
    let removed = 0;
    setState((value) => ({
      ...value,
      phase: "delete",
      error: "",
      removed: null,
    }));
    try {
      const cleared = await historyApi.clear(account, request.signal);
      removed = cleared.removed;
      if (!request.active()) return false;
      outcome = "accepted";
      setState((value) => ({
        ...value,
        phase: null,
        error: "",
        removed: cleared.removed,
      }));
      return true;
    } catch (error) {
      if (request.active()) {
        outcome = "failed";
        setState((value) => ({
          ...value,
          phase: null,
          error: errorMessage(error),
        }));
      }
      return false;
    } finally {
      log.info("history_delete.finished", {
        request_id: requestId,
        reason: outcome,
        count: removed,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }, [account, scope]);

  return {
    loaded: state.enabled !== null,
    enabled: state.enabled ?? true,
    phase: state.phase,
    error: state.error,
    removed: state.removed,
    setEnabled,
    clear,
    reload: () => setAttempt((count) => count + 1),
  };
}
