import { useCallback, useEffect, useState } from "react";
import { errorMessage } from "../../../shared/format";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { log } from "../../../shared/log";
import { sourcePreferenceApi } from "../api/sourcePreferenceApi";

type State = {
  enabled: boolean | null;
  phase: "load" | "save" | null;
  error: string;
};

const initial: State = { enabled: null, phase: "load", error: "" };

export function useBackgroundPreparation(account: string) {
  const scope = useRequestScope(account);
  const [state, setState] = useState<State>(initial);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    const captured = scope.capture();
    const controller = new AbortController();
    const signal = AbortSignal.any([captured.signal, controller.signal]);
    setState(initial);
    sourcePreferenceApi
      .background(account, signal)
      .then((preference) => {
        if (captured.active() && !signal.aborted)
          setState({ enabled: preference.enabled, phase: null, error: "" });
      })
      .catch((error: unknown) => {
        if (captured.active() && !signal.aborted)
          setState({ enabled: null, phase: null, error: errorMessage(error) });
      });
    return () => controller.abort();
  }, [account, scope, attempt]);

  const setEnabled = useCallback(
    async (enabled: boolean) => {
      const request = scope.capture();
      const started = performance.now();
      let outcome = "cancelled";
      setState((value) => ({ ...value, phase: "save", error: "" }));
      try {
        const preference = await sourcePreferenceApi.setBackground(
          account,
          enabled,
          request.signal,
        );
        if (!request.active()) return;
        outcome = "accepted";
        setState({ enabled: preference.enabled, phase: null, error: "" });
      } catch (error) {
        if (!request.active()) return;
        outcome = "failed";
        setState((value) => ({
          ...value,
          phase: null,
          error: errorMessage(error),
        }));
      } finally {
        log.info("source_preferences.background_saved", {
          reason: outcome,
          enabled,
          duration_ms: Math.round(performance.now() - started),
        });
      }
    },
    [account, scope],
  );

  return {
    enabled: state.enabled ?? false,
    loaded: state.enabled !== null,
    phase: state.phase,
    error: state.error,
    setEnabled,
    reload: () => setAttempt((value) => value + 1),
  };
}
