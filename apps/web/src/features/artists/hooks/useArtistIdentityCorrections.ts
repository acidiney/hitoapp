import { useEffect, useState } from "react";
import { artistCorrectionApi } from "../api/artistCorrectionApi";
import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { useRetainedCommand } from "./useRetainedCommand";
import type {
  ArtistIdentityGroup,
  ArtistIdentityGroupChanges,
  SharedArtistCorrections,
  SplitDestination,
} from "../../../shared/types";

export type Selection = {
  recordings: string[];
  releases: string[];
  destination: SplitDestination;
};
type State = {
  key: string;
  changes: ArtistIdentityGroupChanges | null;
  groups: ArtistIdentityGroup[];
  shared: SharedArtistCorrections | null;
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
  const state = snapshot.key === key ? snapshot : blank(key);

  async function load(signal: AbortSignal) {
    const [changes, groups, shared] = await Promise.all([
      artistCorrectionApi.changes(account, group, signal),
      artistCorrectionApi.groups(account, signal),
      admin ? artistCorrectionApi.shared(account, signal) : null,
    ]);
    signal.throwIfAborted();
    if (changes.group_id !== group || !Number.isSafeInteger(changes.revision))
      throw new Error("Invalid artist identity change state");
    setSnapshot({
      key,
      changes,
      groups: groups.items.filter((item) => item.id !== group),
      shared,
      loadError: "",
    });
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

  const command = useRetainedCommand(key, load, onChanged);
  const { changes, shared } = state;
  const latest = changes?.latest;
  const send = artistCorrectionApi;

  return {
    changes,
    groups: state.groups,
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
  return { key, changes: null, groups: [], shared: null, loadError: "" };
}
