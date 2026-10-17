import { useCallback, useEffect, useMemo, useState } from "react";
import { libraryApi } from "../api";
import { HttpError } from "../http";
import { log } from "../log";
import type { Source } from "../types";
import { useRequestScope } from "./useRequestScope";

type View = {
  sources: Source[] | null;
  pending: boolean;
  verified: boolean;
  error: string;
  revision: number;
};
type Owner = {
  view: View;
  generation: number;
  requested: boolean;
  choiceRevision: number | undefined;
  read: AbortController | null;
  confirmations: Set<AbortController>;
};

function checkSources(sources: Source[], recording: string) {
  if (
    !Array.isArray(sources) ||
    !sources.every(
      (source) =>
        typeof source?.id === "string" &&
        source.id.length > 0 &&
        source.recording_id === recording &&
        typeof source.ready === "boolean" &&
        typeof source.can_prepare === "boolean" &&
        typeof source.label === "string" &&
        source.label.trim().length > 0 &&
        source.label.length <= 256,
    )
  )
    throw new Error("Hito returned invalid audio availability");
}

export function useSourceAvailability(
  account: string,
  recording: string,
  choiceRevision?: number,
) {
  const scope = useRequestScope(JSON.stringify([account, recording]));
  const owner = useMemo<Owner>(
    () => ({
      view: {
        sources: null,
        pending: false,
        verified: false,
        error: "",
        revision: 0,
      },
      generation: 0,
      requested: false,
      choiceRevision,
      read: null,
      confirmations: new Set(),
    }),
    [scope],
  );
  const [state, setState] = useState<{ owner: Owner; view: View } | null>(null);
  const current = state?.owner === owner ? state.view : owner.view;
  const choiceChanged =
    choiceRevision !== undefined &&
    owner.choiceRevision !== undefined &&
    choiceRevision !== owner.choiceRevision;

  function publish(update: Partial<View>) {
    owner.view = { ...owner.view, ...update };
    setState({ owner, view: owner.view });
  }

  function cancelConfirmations() {
    for (const controller of owner.confirmations) controller.abort();
    owner.confirmations.clear();
  }

  async function read(refresh: boolean): Promise<void> {
    const request = scope.capture();
    if (!request.active() || owner.read) return;
    cancelConfirmations();
    const generation = ++owner.generation;
    const controller = new AbortController();
    const signal = AbortSignal.any([request.signal, controller.signal]);
    const active = () =>
      request.active() &&
      !signal.aborted &&
      owner.read === controller &&
      owner.generation === generation;
    owner.read = controller;
    owner.requested = true;
    publish({ pending: true, verified: false, error: "" });
    const started = performance.now();
    const fields = {
      account_id: account,
      recording_id: recording,
      action: refresh ? "refresh" : "read",
    };
    log.debug("source_availability.check_started", fields);
    try {
      const sources = await (refresh
        ? libraryApi.refreshSources(account, recording, signal)
        : libraryApi.sources(account, recording, signal));
      if (!active()) return;
      checkSources(sources, recording);
      publish({
        sources,
        pending: false,
        verified: true,
        error: "",
        revision: generation,
      });
      log.debug("source_availability.checked", {
        ...fields,
        mode: sources.some((source) => source.ready) ? "ready" : "unavailable",
        duration_ms: Math.round(performance.now() - started),
      });
    } catch (error) {
      if (active()) {
        publish({
          pending: false,
          verified: false,
          error:
            error instanceof HttpError && error.status === 409
              ? "Service temporarily busy."
              : "Hito could not check audio availability. Try again.",
        });
        log.warn("source_availability.check_failed", {
          ...fields,
          reason: error instanceof HttpError ? "http" : "request",
          status: error instanceof HttpError ? error.status : undefined,
          duration_ms: Math.round(performance.now() - started),
        });
      }
    } finally {
      if (owner.read === controller) owner.read = null;
      if (!request.active() || signal.aborted)
        log.debug("source_availability.check_cancelled", {
          ...fields,
          duration_ms: Math.round(performance.now() - started),
        });
    }
  }

  const confirmReady = useCallback(
    async (source: string, signal: AbortSignal) => {
      const request = scope.capture();
      if (!request.active() || owner.read) return false;
      const generation = owner.generation;
      const controller = new AbortController();
      owner.confirmations.add(controller);
      owner.requested = true;
      const combined = AbortSignal.any([
        request.signal,
        signal,
        controller.signal,
      ]);
      const active = () =>
        request.active() &&
        !combined.aborted &&
        owner.generation === generation &&
        !owner.read &&
        owner.confirmations.has(controller);
      const started = performance.now();
      const fields = {
        account_id: account,
        recording_id: recording,
        source_id: source,
      };
      try {
        const sources = await libraryApi.sources(account, recording, combined);
        if (!active()) return false;
        checkSources(sources, recording);
        const ready = sources.some(
          (candidate) => candidate.id === source && candidate.ready,
        );
        publish({ sources, pending: false, verified: true, error: "" });
        log.debug("source_preparation.audio_checked", {
          ...fields,
          mode: ready ? "ready" : "unavailable",
          duration_ms: Math.round(performance.now() - started),
        });
        return ready;
      } catch (error) {
        if (!active()) return false;
        publish({ verified: false });
        log.warn("source_preparation.audio_check_failed", {
          ...fields,
          reason: error instanceof HttpError ? "http" : "request",
          status: error instanceof HttpError ? error.status : undefined,
          duration_ms: Math.round(performance.now() - started),
        });
        throw error;
      } finally {
        owner.confirmations.delete(controller);
      }
    },
    [owner, scope, account, recording],
  );

  useEffect(() => {
    if (choiceRevision === undefined) return;
    const previous = owner.choiceRevision;
    owner.choiceRevision = choiceRevision;
    if (previous === undefined || previous === choiceRevision) return;
    const observed = owner.requested;
    owner.generation++;
    owner.read?.abort();
    owner.read = null;
    cancelConfirmations();
    publish({ pending: false, verified: false, error: "" });
    log.debug("source_availability.choice_changed", {
      account_id: account,
      recording_id: recording,
      content_revision: choiceRevision,
      mode: observed ? "rechecking" : "unrequested",
    });
    if (observed) void read(false);
  }, [owner, choiceRevision]);

  useEffect(
    () => () => {
      owner.read?.abort();
      owner.read = null;
      cancelConfirmations();
    },
    [owner],
  );

  return {
    sources: current.sources,
    pending:
      current.pending ||
      (choiceChanged && (current.sources !== null || owner.read !== null)),
    verified: current.verified && !choiceChanged,
    error: current.error,
    revision: current.revision,
    check: () => read(true),
    confirmReady,
  };
}
