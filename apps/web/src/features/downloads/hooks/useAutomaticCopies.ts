import { useCallback, useEffect, useState } from "react";
import type { Schema } from "../../../shared/schema";
import { errorMessage } from "../../../shared/format";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { log } from "../../../shared/log";
import { automaticCopiesApi } from "../api/automaticCopiesApi";

type State = {
  copies: Schema<"AutomaticCopies"> | null;
  phase: "load" | "save" | null;
  error: string;
};

const initial: State = { copies: null, phase: "load", error: "" };

export function useAutomaticCopies(account: string) {
  const scope = useRequestScope(account);
  const [state, setState] = useState<State>(initial);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    const captured = scope.capture();
    const controller = new AbortController();
    const signal = AbortSignal.any([captured.signal, controller.signal]);
    setState(initial);
    automaticCopiesApi
      .status(account, signal)
      .then((copies) => {
        if (captured.active() && !signal.aborted)
          setState({ copies, phase: null, error: "" });
      })
      .catch((error: unknown) => {
        if (captured.active() && !signal.aborted)
          setState({ copies: null, phase: null, error: errorMessage(error) });
      });
    return () => controller.abort();
  }, [account, scope, attempt]);

  const setKeep = useCallback(
    async (keep: boolean) => {
      const request = scope.capture();
      const started = performance.now();
      let outcome = "cancelled";
      setState((value) => ({ ...value, phase: "save", error: "" }));
      try {
        const copies = await automaticCopiesApi.setKeep(
          account,
          keep,
          request.signal,
        );
        if (!request.active()) return;
        outcome = "accepted";
        setState({ copies, phase: null, error: "" });
      } catch (error) {
        if (!request.active()) return;
        outcome = "failed";
        setState((value) => ({
          ...value,
          phase: null,
          error: errorMessage(error),
        }));
      } finally {
        log.info("automatic_copies.keep_saved", {
          reason: outcome,
          enabled: keep,
          duration_ms: Math.round(performance.now() - started),
        });
      }
    },
    [account, scope],
  );

  return {
    copies: state.copies,
    phase: state.phase,
    error: state.error,
    setKeep,
    reload: () => setAttempt((value) => value + 1),
  };
}
