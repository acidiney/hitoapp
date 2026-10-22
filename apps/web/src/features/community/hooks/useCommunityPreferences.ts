import { useEffect, useMemo, useState } from "react";
import { errorMessage } from "../../../shared/format";
import { communityApi } from "../api/communityApi";
import { log } from "../../../shared/log";
import type {
  CommunityPreferences,
  UpdateCommunityPreferences,
} from "../../../shared/types";
import { createUuid } from "../../../shared/uuid";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";

type Phase = "load" | "save" | null;
type Owner = { generation: number; pending: Phase };
type Snapshot = {
  owner: Owner;
  accepted: CommunityPreferences | null;
  draft: UpdateCommunityPreferences | null;
  pending: Phase;
  error: string;
  saved: boolean;
};

function empty(owner: Owner): Snapshot {
  return {
    owner,
    accepted: null,
    draft: null,
    pending: "load",
    error: "",
    saved: false,
  };
}

export function useCommunityPreferences(account: string) {
  const scope = useRequestScope(account);
  const owner = useMemo<Owner>(
    () => ({ generation: 0, pending: "load" }),
    [scope],
  );
  const [snapshot, setSnapshot] = useState(() => empty(owner));
  const [reloadVersion, setReloadVersion] = useState(0);
  const current = snapshot.owner === owner ? snapshot : empty(owner);
  const dirty =
    current.accepted !== null &&
    current.draft !== null &&
    (current.accepted.playback_reporting !== current.draft.playback_reporting ||
      current.accepted.metadata_corrections !==
        current.draft.metadata_corrections);

  useEffect(() => {
    const captured = scope.capture();
    const controller = new AbortController();
    const signal = AbortSignal.any([captured.signal, controller.signal]);
    const generation = ++owner.generation;
    const active = () =>
      captured.active() && !signal.aborted && owner.generation === generation;
    owner.pending = "load";
    const requestId = createUuid();
    const started = performance.now();
    let outcome = "cancelled";
    setSnapshot(empty(owner));
    void communityApi
      .preferences(account, signal)
      .then((accepted) => {
        if (!active()) return;
        outcome = "accepted";
        owner.pending = null;
        setSnapshot({
          owner,
          accepted,
          draft: { ...accepted },
          pending: null,
          error: "",
          saved: false,
        });
      })
      .catch((error: unknown) => {
        if (!active()) return;
        outcome = "failed";
        owner.pending = null;
        setSnapshot({
          ...empty(owner),
          pending: null,
          error: errorMessage(error),
        });
      })
      .finally(() => {
        log.debug("community_preferences.finished", {
          action: "load",
          request_id: requestId,
          reason: outcome,
          duration_ms: Math.round(performance.now() - started),
        });
      });
    return () => controller.abort();
  }, [account, owner, scope, reloadVersion]);

  function edit(
    field: "playback_reporting" | "metadata_corrections",
    enabled: boolean,
  ) {
    if (!scope.capture().active() || owner.pending !== null) return;
    setSnapshot((value) =>
      value.owner === owner && value.draft !== null
        ? {
            ...value,
            draft: { ...value.draft, [field]: enabled },
            saved: false,
          }
        : value,
    );
  }

  function cancel() {
    if (!scope.capture().active() || owner.pending !== null) return;
    setSnapshot((value) =>
      value.owner === owner && value.accepted !== null
        ? { ...value, draft: { ...value.accepted }, error: "", saved: false }
        : value,
    );
  }

  async function save() {
    const captured = scope.capture();
    if (
      !captured.active() ||
      owner.pending !== null ||
      !dirty ||
      !current.draft
    )
      return;
    owner.pending = "save";
    const generation = ++owner.generation;
    const active = () => captured.active() && owner.generation === generation;
    const requestId = createUuid();
    const started = performance.now();
    let outcome = "cancelled";
    setSnapshot((value) =>
      value.owner === owner
        ? { ...value, pending: "save", error: "", saved: false }
        : value,
    );
    try {
      const accepted: CommunityPreferences =
        await communityApi.updatePreferences(
          account,
          current.draft,
          captured.signal,
        );
      if (!active()) return;
      outcome = "accepted";
      owner.pending = null;
      setSnapshot({
        owner,
        accepted,
        draft: { ...accepted },
        pending: null,
        error: "",
        saved: true,
      });
    } catch (error) {
      if (!active()) return;
      outcome = "failed";
      owner.pending = null;
      setSnapshot((value) =>
        value.owner === owner
          ? {
              ...value,
              pending: null,
              error: errorMessage(error),
              saved: false,
            }
          : value,
      );
    } finally {
      const record =
        outcome === "accepted"
          ? log.info
          : outcome === "failed"
            ? log.warn
            : log.debug;
      record("community_preferences.finished", {
        action: "save",
        request_id: requestId,
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  function reload() {
    if (!scope.capture().active() || owner.pending !== null) return;
    owner.pending = "load";
    owner.generation++;
    setSnapshot(empty(owner));
    setReloadVersion((value) => value + 1);
  }

  return {
    loaded: current.accepted !== null,
    playbackReporting: current.draft?.playback_reporting ?? false,
    metadataCorrections: current.draft?.metadata_corrections ?? false,
    pending: current.pending,
    error: current.error,
    saved: current.saved,
    dirty,
    edit,
    cancel,
    save,
    reload,
  };
}
