import { useMemo, useState } from "react";
import { HttpError } from "../../../shared/http";
import { log } from "../../../shared/log";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";

export type Attempt = {
  kind: string;
  requestId: string;
  notice: string;
  send(signal: AbortSignal): Promise<{ request_id: string }>;
};
type Owner = { controller: AbortController | null; attempt: Attempt | null };
type View = {
  owner: Owner;
  pending: string | null;
  retrying: boolean;
  error: string;
  notice: string;
};

const deadlineMs = 15000;

function empty(owner: Owner): View {
  return { owner, pending: null, retrying: false, error: "", notice: "" };
}

export function useRetainedCommand(
  key: string,
  reload: (signal: AbortSignal) => Promise<void>,
  accepted: () => void,
) {
  const scope = useRequestScope(key);
  const owner = useMemo<Owner>(
    () => ({ controller: null, attempt: null }),
    [scope],
  );
  const [snapshot, setSnapshot] = useState(() => empty(owner));
  const view = snapshot.owner === owner ? snapshot : empty(owner);

  function update(change: Partial<View>) {
    setSnapshot((value) => ({
      ...(value.owner === owner ? value : empty(owner)),
      ...change,
    }));
  }

  async function run(kind: string, make: () => Attempt | null) {
    const captured = scope.capture();
    if (!captured.active() || owner.controller) return;
    if (!owner.attempt) owner.attempt = make();
    const attempt = owner.attempt;
    if (!attempt) return;
    const controller = new AbortController();
    owner.controller = controller;
    const signal = AbortSignal.any([captured.signal, controller.signal]);
    let expired = false;
    const timer = setTimeout(() => {
      expired = true;
      controller.abort();
    }, deadlineMs);
    const started = performance.now();
    let outcome = "cancelled";
    update({ pending: kind, error: "", notice: "" });
    try {
      const receipt = await attempt.send(signal);
      signal.throwIfAborted();
      if (receipt?.request_id !== attempt.requestId)
        throw new Error("Invalid artist correction receipt");
      owner.attempt = null;
      await Promise.allSettled([reload(signal)]);
      accepted();
      outcome = "accepted";
      update({ retrying: false, notice: attempt.notice });
    } catch (error) {
      if (captured.active() && (!signal.aborted || expired)) {
        const rejected =
          error instanceof HttpError &&
          error.status >= 400 &&
          error.status < 500;
        if (rejected) {
          owner.attempt = null;
          await Promise.allSettled([reload(captured.signal)]);
        }
        outcome = expired ? "deadline" : "failed";
        update({
          retrying: owner.attempt !== null,
          error: owner.attempt
            ? "Hito could not confirm the correction. Retry the same request to recover its result."
            : "Hito could not apply the correction. The current state was reloaded; try again.",
        });
      }
    } finally {
      clearTimeout(timer);
      if (owner.controller === controller) {
        owner.controller = null;
        if (captured.active()) update({ pending: null });
      }
      (outcome === "accepted" ? log.info : log.debug)(
        "artist_correction.finished",
        {
          action: kind,
          reason: outcome,
          duration_ms: Math.round(performance.now() - started),
        },
      );
    }
  }

  return {
    pending: view.pending,
    retrying: view.retrying,
    error: view.error,
    notice: view.notice,
    run,
    retry: () =>
      owner.attempt ? run(owner.attempt.kind, () => null) : undefined,
  };
}
