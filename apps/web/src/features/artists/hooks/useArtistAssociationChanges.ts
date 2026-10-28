import { useEffect, useMemo, useState } from "react";
import { artistAssociationApi } from "../api/artistAssociationApi";
import { HttpError } from "../../../shared/http";
import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import type {
  ArtistAssociation,
  ArtistAssociationChangeReceipt,
  UndoRecordingArtistChange,
  UnlinkRecordingArtist,
} from "../../../shared/types";

type Kind = "unlink" | "undo";
type Attempt =
  | { kind: "unlink"; body: UnlinkRecordingArtist }
  | { kind: "undo"; body: UndoRecordingArtistChange };
type Undo = { revision: number; association_id: string };
type Owner = {
  controller: AbortController | null;
  attempt: Attempt | null;
  undo: Undo | null;
};
type View = {
  owner: Owner;
  pending: Kind | null;
  undo: Undo | null;
  retrying: boolean;
  error: string;
};

const deadlineMs = 15000;

function empty(owner: Owner): View {
  return { owner, pending: null, undo: null, retrying: false, error: "" };
}

function validReceipt(
  receipt: ArtistAssociationChangeReceipt,
  recording: string,
  attempt: Attempt,
) {
  const body = attempt.body;
  return (
    receipt?.request_id === body.request_id &&
    receipt.recording_id === recording &&
    receipt.revision === body.expected_revision + 1 &&
    typeof receipt.association_id === "string" &&
    (attempt.kind === "unlink"
      ? receipt.association_id === attempt.body.association_id &&
        receipt.linked === false
      : typeof receipt.linked === "boolean")
  );
}

export function useArtistAssociationChanges(
  account: string,
  recording: string,
  onChanged: () => Promise<void>,
) {
  const scope = useRequestScope(JSON.stringify([account, recording]));
  const owner = useMemo<Owner>(
    () => ({ controller: null, attempt: null, undo: null }),
    [scope],
  );
  const [snapshot, setSnapshot] = useState(() => empty(owner));
  const current = snapshot.owner === owner ? snapshot : empty(owner);

  async function perform(kind: Kind, association?: ArtistAssociation) {
    const captured = scope.capture();
    if (!captured.active() || owner.controller) return;
    if (
      !owner.attempt &&
      (kind === "undo"
        ? !owner.undo
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
          const state = await artistAssociationApi.changes(
            account,
            recording,
            signal,
          );
          signal.throwIfAborted();
          owner.attempt = {
            kind,
            body: Object.freeze({
              request_id: requestId,
              expected_revision: state.revision,
              association_id: association.id,
            }),
          };
        } else if (owner.undo) {
          owner.attempt = {
            kind: "undo",
            body: Object.freeze({
              request_id: requestId,
              expected_revision: owner.undo.revision,
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
      owner.undo = {
        revision: receipt.revision,
        association_id: receipt.association_id,
      };
      await onChanged();
      if (!active()) return;
      reason = "accepted";
      setSnapshot((value) => ({
        ...(value.owner === owner ? value : empty(owner)),
        undo: owner.undo,
        retrying: false,
        error: "",
      }));
    } catch (error) {
      if (active() && (!signal.aborted || expired)) {
        if (
          error instanceof HttpError &&
          error.status >= 400 &&
          error.status < 500
        )
          owner.attempt = null;
        reason = expired ? "deadline" : "failed";
        setSnapshot((value) => ({
          ...(value.owner === owner ? value : empty(owner)),
          undo: owner.undo,
          retrying: owner.attempt !== null,
          error: owner.attempt
            ? "Hito could not confirm the artist review change. Retry the same request to recover its result."
            : "Hito could not change the saved artist reviews. Reload them and try again.",
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

  useEffect(() => () => owner.controller?.abort(), [owner]);

  return {
    pending: current.pending,
    canUndo:
      current.undo !== null && current.pending === null && !current.retrying,
    retrying: current.retrying,
    error: current.error,
    unlink: (association: ArtistAssociation) => perform("unlink", association),
    undo: () => perform("undo"),
    retry: () =>
      owner.attempt ? perform(owner.attempt.kind) : Promise.resolve(),
  };
}
