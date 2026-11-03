import type { Schema } from "../../../shared/schema";
import { useCallback, useEffect, useState } from "react";
import { errorMessage } from "../../../shared/format";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { accountApi } from "../api/accountApi";

type Loaded = {
  sessions: Schema<"SessionInfo">[] | null;
  error: string;
};

export function useOpenSessions(account: string, version: number) {
  const scope = useRequestScope(account);
  const [loaded, setLoaded] = useState<Loaded>({ sessions: null, error: "" });
  const [attempt, setAttempt] = useState(0);
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState("");

  useEffect(() => {
    const captured = scope.capture();
    const controller = new AbortController();
    const signal = AbortSignal.any([captured.signal, controller.signal]);
    setLoaded((previous) => ({ sessions: previous.sessions, error: "" }));
    accountApi
      .sessions(account, signal)
      .then((sessions) => {
        if (captured.active() && !signal.aborted)
          setLoaded({ sessions, error: "" });
      })
      .catch((error: unknown) => {
        if (captured.active() && !signal.aborted)
          setLoaded({ sessions: null, error: errorMessage(error) });
      });
    return () => controller.abort();
  }, [account, scope, version, attempt]);

  const run = useCallback(
    async (
      send: (signal: AbortSignal) => Promise<unknown>,
      apply: (sessions: Schema<"SessionInfo">[]) => Schema<"SessionInfo">[],
    ) => {
      const request = scope.capture();
      setBusy(true);
      setActionError("");
      try {
        await send(request.signal);
        if (request.active())
          setLoaded((previous) => ({
            sessions: previous.sessions && apply(previous.sessions),
            error: "",
          }));
      } catch (error) {
        if (request.active()) setActionError(errorMessage(error));
      } finally {
        if (request.active()) setBusy(false);
      }
    },
    [scope],
  );

  const revoke = useCallback(
    (id: string) =>
      run(
        (signal) => accountApi.revokeSession(account, id, signal),
        (sessions) => sessions.filter((session) => session.id !== id),
      ),
    [account, run],
  );

  const revokeOthers = useCallback(
    () =>
      run(
        (signal) => accountApi.revokeOthers(account, signal),
        (sessions) => sessions.filter((session) => session.current),
      ),
    [account, run],
  );

  return {
    sessions: loaded.sessions,
    error: loaded.error || actionError,
    busy,
    revoke,
    revokeOthers,
    retry: () => setAttempt((count) => count + 1),
  };
}
