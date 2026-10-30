import type { Schema } from "../../../shared/schema";
import { useEffect, useState } from "react";
import { artistCorrectionApi } from "../api/artistCorrectionApi";
import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { useRetainedCommand } from "./useRetainedCommand";

export type Selection = {
  recordings: string[];
  releases: string[];
  destination: Schema<"SplitDestination">;
};
type State = {
  key: string;
  changes: Schema<"ArtistIdentityGroupChanges"> | null;
  groups: Schema<"ArtistIdentityGroup">[];
  groupTotal: number;
  shared: Schema<"SharedArtistCorrections"> | null;
  loadError: string;
};

export function useArtistIdentityCorrections(
  account: string,
  group: string,
  admin: boolean,
  opened: boolean,
  onChanged: () => void,
) {
  const key = JSON.stringify([account, group, admin]);
  const scope = useRequestScope(key);
  const [snapshot, setSnapshot] = useState<State>(() => blank(key));
  const [destination, setDestination] = useState({ query: "", offset: 0 });
  const state = snapshot.key === key ? snapshot : blank(key);

  async function load(signal: AbortSignal) {
    const [changes, shared] = await Promise.all([
      artistCorrectionApi.changes(account, group, signal),
      admin ? artistCorrectionApi.shared(account, signal) : null,
    ]);
    signal.throwIfAborted();
    if (changes.group_id !== group || !Number.isSafeInteger(changes.revision))
      throw new Error("Invalid artist identity change state");
    setSnapshot((value) => ({
      ...(value.key === key ? value : blank(key)),
      changes,
      shared,
      loadError: "",
    }));
  }

  async function loadGroups(signal: AbortSignal) {
    const groups = await artistCorrectionApi.groups(
      account,
      signal,
      destination.query,
      destination.offset,
    );
    signal.throwIfAborted();
    setSnapshot((value) => ({
      ...(value.key === key ? value : blank(key)),
      groups: groups.items.filter((item) => item.id !== group),
      groupTotal: groups.total,
    }));
  }

  useEffect(() => {
    if (!opened) return;
    const captured = scope.capture();
    load(captured.signal).catch((error) => {
      if (!captured.active()) return;
      setSnapshot((value) => ({
        ...(value.key === key ? value : blank(key)),
        loadError: "Hito could not read this artist's corrections.",
      }));
      log.warn("artist_identity.corrections_load_failed", {
        reason: error instanceof Error ? error.name : "unknown",
      });
    });
  }, [opened, scope]);

  useEffect(() => {
    if (!opened) return;
    const captured = scope.capture();
    loadGroups(captured.signal).catch(() => {
      if (captured.active())
        setSnapshot((value) => ({
          ...(value.key === key ? value : blank(key)),
          loadError: "Hito could not read this artist's corrections.",
        }));
    });
  }, [opened, scope, destination]);

  const command = useRetainedCommand(key, load, onChanged);
  const { changes, shared } = state;
  const latest = changes?.latest;
  const send = artistCorrectionApi;

  return {
    changes,
    groups: state.groups,
    groupTotal: state.groupTotal,
    groupPage: destination,
    findGroups: (query: string) => setDestination({ query, offset: 0 }),
    moveGroupPage: (step: number) =>
      setDestination((value) => ({
        ...value,
        offset: Math.max(0, value.offset + step * 20),
      })),
    shared,
    pending: command.pending,
    retrying: command.retrying,
    error: command.error || state.loadError,
    notice: command.notice,
    retry: command.retry,
    split: (selection: Selection) =>
      command.run("split", () => {
        if (!changes) return null;
        const body = Object.freeze({
          request_id: createUuid(),
          expected_revision: changes.revision,
          recordings: selection.recordings,
          releases: selection.releases,
          destination: selection.destination,
        });
        return {
          kind: "split",
          requestId: body.request_id,
          notice: "The selected tracks were moved. You can undo this change.",
          send: (signal) => send.split(account, group, body, signal),
        };
      }),
    undo: () =>
      command.run("undo", () => {
        if (!changes || !latest?.undoable) return null;
        const body = Object.freeze({
          request_id: createUuid(),
          expected_revision: changes.revision,
        });
        return {
          kind: "undo",
          requestId: body.request_id,
          notice: "The change was undone.",
          send: (signal) => send.undo(account, group, body, signal),
        };
      }),
    share: () =>
      command.run("share", () => {
        if (!latest?.shareable || !shared) return null;
        const body = Object.freeze({
          request_id: createUuid(),
          expected_revision: shared.revision,
          source: Object.freeze({
            kind: "split" as const,
            group_id: group,
            revision: latest.revision,
          }),
        });
        return {
          kind: "share",
          requestId: body.request_id,
          notice: "The correction now applies to every account.",
          send: (signal) => send.promote(account, body, signal),
        };
      }),
    withdraw: () =>
      command.run("withdraw", () => {
        const applied = latest?.shared;
        if (!applied) return null;
        const body = Object.freeze({
          request_id: createUuid(),
          expected_revision: applied.revision,
        });
        return {
          kind: "withdraw",
          requestId: body.request_id,
          notice: "The shared correction was withdrawn.",
          send: (signal) => send.undoShared(account, applied.id, body, signal),
        };
      }),
  };
}

function blank(key: string): State {
  return {
    key,
    changes: null,
    groups: [],
    groupTotal: 0,
    shared: null,
    loadError: "",
  };
}
