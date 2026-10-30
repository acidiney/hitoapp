import type { Schema } from "../../../shared/schema";
import { useEffect, useMemo, useState } from "react";
import { artistAssociationApi } from "../api/artistAssociationApi";
import { HttpError } from "../../../shared/http";
import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";

type Kind = "unlink" | "undo";
type Attempt =
  | {
      kind: "unlink";
      body: Schema<"UnlinkRecordingArtist">;
      association: string;
    }
  | {
      kind: "undo";
      body: Schema<"UndoRecordingArtistChange">;
      association: string;
    };
type Owner = {
  controller: AbortController | null;
  attempt: Attempt | null;
  state: Schema<"ArtistAssociationChanges"> | null;
};
type View = {
  owner: Owner;
  pending: Kind | null;
  state: Schema<"ArtistAssociationChanges"> | null;
  retrying: boolean;
  error: string;
};

const deadlineMs = 15000;

function empty(owner: Owner): View {
  return {
    owner,
    pending: null,
    state: owner.state,
    retrying: false,
    error: "",
  };
}

function validReceipt(
  receipt: Schema<"ArtistAssociationChangeReceipt">,
  recording: string,
  attempt: Attempt,
) {
  return (
    receipt?.request_id === attempt.body.request_id &&
    receipt.recording_id === recording &&
    receipt.revision === attempt.body.expected_revision + 1 &&
    receipt.association_id === attempt.association &&
    (attempt.kind === "unlink" ? receipt.linked === false : true)
  );
}

export function useArtistAssociationChanges(
  account: string,
  detail: Schema<"RecordingMetadata">,
  opened: boolean,
  onChanged: () => Promise<void>,
) {
  const recording = detail.recording_id;
  const scope = useRequestScope(
    JSON.stringify([
      account,
      recording,
      detail.selected_observation?.id,
      detail.artist.operation_id,
      detail.artist.value,
    ]),
  );
  const owner = useMemo<Owner>(
    () => ({ controller: null, attempt: null, state: null }),
    [scope],
  );
  const [snapshot, setSnapshot] = useState(() => empty(owner));
  const current = snapshot.owner === owner ? snapshot : empty(owner);

  async function readState(signal: AbortSignal) {
    const state = await artistAssociationApi.changes(
      account,
      recording,
      signal,
    );
    signal.throwIfAborted();
    if (
      state.recording_id !== recording ||
      !Number.isSafeInteger(state.revision)
    )
      throw new Error("Invalid artist review change state");
    owner.state = state;
    setSnapshot((value) => ({
      ...(value.owner === owner ? value : empty(owner)),
      state,
    }));
  }

  async function perform(
    kind: Kind,
    association?: Schema<"ArtistAssociation">,
  ) {
    const captured = scope.capture();
    const state = owner.state;
    if (!captured.active() || owner.controller || !state) return;
    if (
      !owner.attempt &&
      (kind === "undo"
        ? !state.undo
        : !association ||
          association.account_id !== account ||
          association.recording_id !== recording)
    )
      return;
    const controller = new AbortController();
    owner.controller = controller;
    const signal = AbortSignal.any([captured.signal, controller.signal]);
    const active = () => captured.active() && owner.controller === controller;
    let expired = false;
    let reason = "cancelled";
    const requestId = owner.attempt?.body.request_id ?? createUuid();
    const started = performance.now();
    const timer = setTimeout(() => {
      expired = true;
      controller.abort();
    }, deadlineMs);
    setSnapshot((value) => ({
      ...(value.owner === owner ? value : empty(owner)),
      pending: kind,
      error: "",
    }));
    try {
      if (!owner.attempt) {
        if (kind === "unlink" && association) {
          owner.attempt = {
            kind,
            association: association.id,
            body: Object.freeze({
              request_id: requestId,
              expected_revision: state.revision,
              association_id: association.id,
            }),
          };
        } else if (state.undo) {
          owner.attempt = {
            kind: "undo",
            association: state.undo.association_id,
            body: Object.freeze({
              request_id: requestId,
              expected_revision: state.revision,
            }),
          };
        }
      }
      const attempt = owner.attempt;
      if (!attempt) throw new Error("Artist review change is unavailable");
      const receipt =
        attempt.kind === "unlink"
          ? await artistAssociationApi.unlink(
              account,
              recording,
              attempt.body,
              signal,
            )
          : await artistAssociationApi.undo(
              account,
              recording,
              attempt.body,
              signal,
            );
      signal.throwIfAborted();
      if (!validReceipt(receipt, recording, attempt))
        throw new Error("Invalid artist review change receipt");
      owner.attempt = null;
      await Promise.allSettled([readState(signal), onChanged()]);
      if (!active()) return;
      reason = "accepted";
      setSnapshot((value) => ({
        ...(value.owner === owner ? value : empty(owner)),
        retrying: false,
        error: "",
      }));
    } catch (error) {
      if (active() && (!signal.aborted || expired)) {
        const rejected =
          error instanceof HttpError &&
          error.status >= 400 &&
          error.status < 500;
        if (rejected) {
          owner.attempt = null;
          await Promise.allSettled([readState(captured.signal), onChanged()]);
        }
        reason = expired ? "deadline" : "failed";
        setSnapshot((value) => ({
          ...(value.owner === owner ? value : empty(owner)),
          retrying: owner.attempt !== null,
          error: owner.attempt
            ? "Hito could not confirm the artist review change. Retry the same request to recover its result."
            : "Hito could not change the saved artist reviews. They were reloaded; try again.",
        }));
      }
    } finally {
      clearTimeout(timer);
      if (owner.controller === controller) {
        owner.controller = null;
        if (captured.active())
          setSnapshot((value) =>
            value.owner === owner ? { ...value, pending: null } : value,
          );
      }
      const record =
        reason === "failed" || reason === "deadline"
          ? log.warn
          : reason === "accepted"
            ? log.info
            : log.debug;
      record("artist_association.finished", {
        action: kind,
        request_id: requestId,
        reason,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  useEffect(() => {
    if (!opened) return;
    const captured = scope.capture();
    void readState(captured.signal).catch(() => {});
  }, [opened, owner, scope]);

  useEffect(() => () => owner.controller?.abort(), [owner]);

  return {
    state: current.state,
    reload: readState,
    ready: current.state !== null,
    canUndo:
      current.state?.undo != null &&
      current.pending === null &&
      !current.retrying,
    retrying: current.retrying,
    error: current.error,
    unlink: (association: Schema<"ArtistAssociation">) =>
      perform("unlink", association),
    undo: () => perform("undo"),
    retry: () =>
      owner.attempt ? perform(owner.attempt.kind) : Promise.resolve(),
  };
}
