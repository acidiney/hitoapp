import { useCallback, useMemo, useState } from "react";
import { libraryApi } from "../api";
import { HttpError } from "../http";
import { log } from "../log";
import type { Source } from "../types";
import { useRequestScope } from "./useRequestScope";

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

export function useSourceAvailability(account: string, recording: string) {
  const scope = useRequestScope(JSON.stringify([account, recording]));
  const owner = useMemo(() => ({ revision: 0, busy: false }), [scope]);
  const [state, setState] = useState<{
    owner: typeof owner;
    sources: Source[] | null;
    pending: boolean;
    error: string;
    revision: number;
  }>({ owner, sources: null, pending: false, error: "", revision: 0 });
  async function check() {
    if (owner.busy) return;
    const request = scope.capture();
    if (!request.active()) return;
    owner.busy = true;
    owner.revision++;
    setState((current) => ({
      owner,
      sources: current.owner === owner ? current.sources : null,
      pending: true,
      error: "",
      revision: current.owner === owner ? current.revision : 0,
    }));
    try {
      const sources = await libraryApi.refreshSources(
        account,
        recording,
        request.signal,
      );
      checkSources(sources, recording);
      if (request.active())
        setState({
          owner,
          sources,
          pending: false,
          error: "",
          revision: owner.revision,
        });
    } catch (error) {
      if (request.active())
        setState((current) => ({
          owner,
          sources: current.owner === owner ? current.sources : null,
          pending: false,
          revision: current.owner === owner ? current.revision : 0,
          error:
            error instanceof HttpError && error.status === 409
              ? "Service temporarily busy."
              : "Hito could not check audio availability. Try again.",
        }));
    } finally {
      owner.busy = false;
    }
  }
  const confirmReady = useCallback(
    async (source: string, signal: AbortSignal) => {
      const request = scope.capture();
      const revision = owner.revision;
      const started = performance.now();
      const combined = AbortSignal.any([request.signal, signal]);
      const sources = await libraryApi.sources(account, recording, combined);
      checkSources(sources, recording);
      if (
        !request.active() ||
        combined.aborted ||
        owner.revision !== revision ||
        owner.busy
      )
        return false;
      const ready = sources.some(
        (candidate) =>
          candidate.id === source &&
          candidate.recording_id === recording &&
          candidate.ready === true,
      );
      setState((current) => ({
        owner,
        sources,
        pending: false,
        error: "",
        revision: current.owner === owner ? current.revision : 0,
      }));
      log.debug("source_preparation.audio_checked", {
        account_id: account,
        recording_id: recording,
        source_id: source,
        mode: ready ? "ready" : "unavailable",
        duration_ms: Math.round(performance.now() - started),
      });
      return ready;
    },
    [owner, scope, account, recording],
  );
  return {
    sources: state.owner === owner ? state.sources : null,
    pending: state.owner === owner && state.pending,
    error: state.owner === owner ? state.error : "",
    revision: state.owner === owner ? state.revision : 0,
    check,
    confirmReady,
  };
}
