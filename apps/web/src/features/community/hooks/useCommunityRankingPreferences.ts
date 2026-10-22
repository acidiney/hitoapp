import { useEffect, useMemo, useState } from "react";
import { HttpError } from "../../../shared/http";
import { communityApi } from "../api/communityApi";
import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import type {
  CommunityRankingPreferences as Preferences,
  UpdateCommunityRankingPreferences as Update,
} from "../../../shared/types";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";

type Phase = "load" | "save";
type Owner = { controller: AbortController | null; pending: Phase | null };
type Snapshot = {
  owner: Owner;
  accepted: Preferences | null;
  draft: Update | null;
  pending: Phase | null;
  error: string;
  stale: boolean;
  saved: boolean;
};

function empty(owner: Owner): Snapshot {
  return {
    owner,
    accepted: null,
    draft: null,
    pending: "load",
    error: "",
    stale: false,
    saved: false,
  };
}

export function useCommunityRankingPreferences(account: string) {
  const scope = useRequestScope(account);
  const owner = useMemo<Owner>(
    () => ({ controller: null, pending: null }),
    [scope],
  );
  const [snapshot, setSnapshot] = useState(() => empty(owner));
  const [reloadVersion, setReloadVersion] = useState(0);
  const current = snapshot.owner === owner ? snapshot : empty(owner);
  const dirty =
    current.accepted !== null &&
    current.draft !== null &&
    current.accepted.use_source_rankings !== current.draft.use_source_rankings;

  async function perform(phase: Phase, update?: Update) {
    const captured = scope.capture();
    if (!captured.active() || (phase === "save" && owner.pending !== null))
      return;
    owner.controller?.abort();
    const controller = new AbortController();
    owner.controller = controller;
    owner.pending = phase;
    const signal = AbortSignal.any([captured.signal, controller.signal]);
    const active = () => captured.active() && owner.controller === controller;
    const started = performance.now();
    const requestId = createUuid();
    let reason = "cancelled";
    let expired = false;
    const timer = setTimeout(() => {
      expired = true;
      controller.abort();
    }, 5000);
    let aborted!: () => void;
    const cancelled = new Promise<never>((_, reject) => {
      aborted = () => reject(new DOMException("Cancelled", "AbortError"));
      signal.addEventListener("abort", aborted, { once: true });
      if (signal.aborted) aborted();
    });
    setSnapshot((value) =>
      phase === "load"
        ? empty(owner)
        : {
            ...value,
            pending: phase,
            saved: false,
            error: "",
            stale: false,
          },
    );
    try {
      const accepted: Preferences = await Promise.race([
        communityApi.rankingPreferences(account, update ?? null, signal),
        cancelled,
      ]);
      if (!active() || signal.aborted) return;
      if (
        !Number.isSafeInteger(accepted.revision) ||
        accepted.revision < 0 ||
        typeof accepted.use_source_rankings !== "boolean"
      )
        throw new Error("Invalid ranking preference response");
      reason = "accepted";
      setSnapshot({
        owner,
        accepted,
        draft: { ...accepted },
        pending: null,
        error: "",
        stale: false,
        saved: phase === "save",
      });
    } catch (error) {
      if (!active() || (signal.aborted && !expired)) return;
      reason = expired ? "deadline" : "failed";
      setSnapshot((value) => ({
        ...value,
        pending: null,
        saved: false,
        error: expired
          ? "Source ranking preferences took too long. Reload them to try again."
          : error instanceof HttpError
            ? error.message
            : "Hito could not complete the source ranking preference request. Try again.",
        stale: error instanceof HttpError && error.status === 409,
      }));
    } finally {
      clearTimeout(timer);
      signal.removeEventListener("abort", aborted);
      if (owner.controller === controller) {
        owner.controller = null;
        owner.pending = null;
      }
      const record =
        reason === "accepted" && phase === "save"
          ? log.info
          : reason === "failed" || reason === "deadline"
            ? log.warn
            : log.debug;
      record("community_ranking_preferences.finished", {
        action: phase,
        request_id: requestId,
        reason,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  useEffect(() => {
    void perform("load");
    return () => owner.controller?.abort();
  }, [account, owner, scope, reloadVersion]);

  function edit(enabled: boolean) {
    if (!scope.capture().active() || owner.pending !== null) return;
    setSnapshot((value) =>
      value.owner === owner && value.draft
        ? {
            ...value,
            draft: { ...value.draft, use_source_rankings: enabled },
            saved: false,
          }
        : value,
    );
  }
  function cancel() {
    if (!scope.capture().active() || owner.pending !== null) return;
    setSnapshot((value) =>
      value.owner === owner && value.accepted
        ? {
            ...value,
            draft: { ...value.accepted },
            saved: false,
          }
        : value,
    );
  }
  function save() {
    if (!current.draft || !dirty || current.stale) return;
    return perform("save", { ...current.draft });
  }
  function reload() {
    if (!scope.capture().active() || owner.pending !== null) return;
    setReloadVersion((value) => value + 1);
  }
  return {
    loaded: current.accepted !== null,
    enabled: current.draft?.use_source_rankings ?? false,
    pending: current.pending,
    error: current.error,
    stale: current.stale,
    saved: current.saved,
    dirty,
    edit,
    cancel,
    save,
    reload,
  };
}
