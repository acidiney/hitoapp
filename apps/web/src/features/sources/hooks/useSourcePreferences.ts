import type { Schema } from "../../../shared/schema";
import { useEffect, useState } from "react";
import { errorMessage } from "../../../shared/format";
import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { pluginsApi } from "../../plugins/api/pluginsApi";
import { sourcePreferenceApi } from "../api/sourcePreferenceApi";

type Entry = Schema<"SourcePreferenceEntry">;
type Draft = { fallback: boolean; entries: Entry[] };
type Phase = "load" | "save" | null;
type Owner = { generation: number; pending: Phase };
type Snapshot = {
  owner: Owner;
  accepted: Schema<"SourcePreferences"> | null;
  draft: Draft | null;
  connections: Schema<"PluginSettings"> | null;
  pending: Phase;
  error: string;
  saved: boolean;
};

export function entryKey(entry: Entry) {
  return entry.kind === "provider" ? entry.installation_id : entry.kind;
}

function empty(owner: Owner): Snapshot {
  return {
    owner,
    accepted: null,
    draft: null,
    connections: null,
    pending: "load",
    error: "",
    saved: false,
  };
}

function draftOf(settings: Schema<"SourcePreferences">): Draft {
  return { fallback: settings.fallback, entries: [...settings.entries] };
}

export function useSourcePreferences(account: string) {
  const scope = useRequestScope(account);
  const [owner] = useState<Owner>(() => ({ generation: 0, pending: "load" }));
  const [snapshot, setSnapshot] = useState(() => empty(owner));
  const [reloads, setReloads] = useState(0);
  const current = snapshot.owner === owner ? snapshot : empty(owner);
  const dirty =
    current.accepted !== null &&
    current.draft !== null &&
    (current.accepted.fallback !== current.draft.fallback ||
      current.accepted.entries.map(entryKey).join() !==
        current.draft.entries.map(entryKey).join());

  useEffect(() => {
    const request = scope.capture();
    const generation = ++owner.generation;
    owner.pending = "load";
    const active = () => request.active() && owner.generation === generation;
    const requestId = createUuid();
    const started = performance.now();
    let reason = "cancelled";
    setSnapshot(empty(owner));
    log.debug("source_preferences.started", {
      action: "load",
      request_id: requestId,
    });
    void Promise.all([
      sourcePreferenceApi.settings(account, request.signal),
      pluginsApi.settings(account, request.signal),
    ])
      .then(([accepted, connections]) => {
        if (!active()) return;
        reason = "accepted";
        owner.pending = null;
        setSnapshot({
          owner,
          accepted,
          draft: draftOf(accepted),
          connections,
          pending: null,
          error: "",
          saved: false,
        });
      })
      .catch((error: unknown) => {
        if (!active()) return;
        reason = "failed";
        owner.pending = null;
        setSnapshot({
          ...empty(owner),
          pending: null,
          error: errorMessage(error),
        });
      })
      .finally(() => {
        log.debug("source_preferences.finished", {
          action: "load",
          request_id: requestId,
          reason,
          duration_ms: Math.round(performance.now() - started),
        });
      });
  }, [account, owner, scope, reloads]);

  function edit(change: (draft: Draft) => Draft) {
    if (!scope.capture().active() || owner.pending !== null) return;
    setSnapshot((value) =>
      value.owner === owner && value.draft !== null
        ? { ...value, draft: change(value.draft), error: "", saved: false }
        : value,
    );
  }

  function move(entry: Entry, direction: -1 | 1) {
    edit((draft) => {
      const from = draft.entries.findIndex(
        (item) => entryKey(item) === entryKey(entry),
      );
      const to = from + direction;
      if (from < 0 || to < 0 || to >= draft.entries.length) return draft;
      const entries = [...draft.entries];
      [entries[from], entries[to]] = [entries[to], entries[from]];
      return { ...draft, entries };
    });
  }

  function setFallback(fallback: boolean) {
    edit((draft) => ({ ...draft, fallback }));
  }

  async function save() {
    const request = scope.capture();
    if (!request.active() || !dirty || !current.draft || owner.pending !== null)
      return false;
    const accepted = current.accepted;
    owner.pending = "save";
    const generation = ++owner.generation;
    const active = () => request.active() && owner.generation === generation;
    const requestId = createUuid();
    const started = performance.now();
    let reason = "cancelled";
    setSnapshot((value) =>
      value.owner === owner
        ? { ...value, pending: "save", error: "", saved: false }
        : value,
    );
    log.debug("source_preferences.started", {
      action: "save",
      request_id: requestId,
    });
    try {
      const saved = await sourcePreferenceApi.update(
        account,
        {
          revision: accepted?.revision ?? 0,
          fallback: current.draft.fallback,
          entries: current.draft.entries,
        },
        request.signal,
      );
      if (!active()) return false;
      reason = "accepted";
      owner.pending = null;
      setSnapshot((value) =>
        value.owner === owner
          ? {
              ...value,
              accepted: saved,
              draft: draftOf(saved),
              pending: null,
              error: "",
              saved: true,
            }
          : value,
      );
      return true;
    } catch (error) {
      if (active()) {
        reason = "failed";
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
      }
      return false;
    } finally {
      const record =
        reason === "accepted"
          ? log.info
          : reason === "failed"
            ? log.warn
            : log.debug;
      record("source_preferences.finished", {
        action: "save",
        request_id: requestId,
        reason,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  return {
    loaded: current.accepted !== null,
    fallback: current.draft?.fallback ?? true,
    entries: current.draft?.entries ?? [],
    connections: current.connections,
    pending: current.pending !== null,
    error: current.error,
    saved: current.saved,
    dirty,
    move,
    setFallback,
    save,
    reload: () => {
      if (!scope.capture().active() || owner.pending !== null) return;
      sourcePreferenceApi.forget(account);
      setReloads((value) => value + 1);
    },
  };
}
