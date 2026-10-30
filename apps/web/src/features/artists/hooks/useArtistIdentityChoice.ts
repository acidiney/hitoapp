import type { Schema } from "../../../shared/schema";
import { useEffect, useMemo, useState } from "react";
import { artistIdentityApi } from "../api/artistIdentityApi";
import { artistReviewApi } from "../api/artistReviewApi";
import { log } from "../../../shared/log";
import { HttpError } from "../../../shared/http";
import { createUuid } from "../../../shared/uuid";

import { useRequestScope } from "../../../shared/hooks/useRequestScope";

type Phase = "read" | "choose" | "reset";
type Attempt =
  | { kind: "choose"; body: Schema<"ChooseRecordingArtistIdentity"> }
  | { kind: "reset"; body: Schema<"ResetRecordingArtistIdentity"> };
type Owner = {
  controller: AbortController | null;
  phase: Phase | null;
  choice: Schema<"RecordingArtistIdentityChoice"> | null;
  attempt: Attempt | null;
};
type View = {
  owner: Owner;
  choice: Schema<"RecordingArtistIdentityChoice"> | null;
  name: string | null;
  pending: Phase | null;
  error: string;
  retrying: boolean;
};

function empty(owner: Owner): View {
  return {
    owner,
    choice: null,
    name: null,
    pending: null,
    error: "",
    retrying: false,
  };
}

export function useArtistIdentityChoice(
  account: string,
  detail: Schema<"RecordingMetadata">,
  opened: boolean,
  association: Schema<"ArtistAssociation"> | undefined,
) {
  const recording = detail.recording_id;
  const scope = useRequestScope(
    JSON.stringify([
      account,
      recording,
      detail.artist.operation_id,
      detail.artist.value,
      detail.selected_observation?.id,
      detail.selected_observation?.source_id,
    ]),
  );
  const owner = useMemo<Owner>(
    () => ({ controller: null, phase: null, choice: null, attempt: null }),
    [scope],
  );
  const [snapshot, setSnapshot] = useState(() => empty(owner));
  const current = snapshot.owner === owner ? snapshot : empty(owner);

  function validateChoice(choice: Schema<"RecordingArtistIdentityChoice">) {
    if (
      !choice ||
      choice.recording_id !== recording ||
      !Number.isSafeInteger(choice.revision) ||
      choice.revision < 0 ||
      !choice.state ||
      !["unselected", "selected", "needs_review"].includes(
        choice.state.status,
      ) ||
      (choice.state.status !== "unselected" &&
        (typeof choice.state.group_id !== "string" ||
          !choice.state.group_id ||
          typeof choice.state.association_id !== "string" ||
          !choice.state.association_id))
    )
      throw new Error("Invalid artist identity response");
    return choice;
  }

  function validateContext(
    context: Schema<"ArtistReviewContext">,
    reviewed: Schema<"ArtistAssociation">,
  ) {
    if (
      context.account_id !== account ||
      context.recording_id !== recording ||
      context.library_id !== reviewed.library_id ||
      context.artist_id !== reviewed.artist_id ||
      context.credited_name !== detail.artist.value ||
      context.artist_operation_id !== (detail.artist.operation_id ?? null) ||
      context.observation_id !== detail.selected_observation?.id ||
      context.source_id !== (detail.selected_observation?.source_id ?? null) ||
      JSON.stringify(context.provider) !== JSON.stringify(reviewed.provider)
    )
      throw new Error("The artist review changed");
  }

  async function perform(phase: Phase, reviewed?: Schema<"ArtistAssociation">) {
    const captured = scope.capture();
    if (!captured.active() || owner.controller) return;
    const choice = owner.choice;
    if (phase === "read" && owner.attempt) return;
    if (
      phase === "reset" &&
      !owner.attempt &&
      (!choice || choice.state.status === "unselected")
    )
      return;
    if (
      phase === "choose" &&
      !owner.attempt &&
      (!choice ||
        !reviewed ||
        reviewed.account_id !== account ||
        reviewed.recording_id !== recording ||
        reviewed.observation_id !== detail.selected_observation?.id ||
        reviewed.source_id !== (detail.selected_observation?.source_id ?? null))
    )
      return;
    const controller = new AbortController();
    owner.controller = controller;
    owner.phase = phase;
    const signal = AbortSignal.any([captured.signal, controller.signal]);
    const active = () => captured.active() && owner.controller === controller;
    let expired = false;
    let reason = "cancelled";
    const requestId = owner.attempt?.body.request_id ?? createUuid();
    const started = performance.now();
    const timer = setTimeout(() => {
      expired = true;
      controller.abort();
    }, 15000);
    let abort!: () => void;
    const cancelled = new Promise<never>((_, reject) => {
      abort = () => reject(new DOMException("Cancelled", "AbortError"));
      signal.addEventListener("abort", abort, { once: true });
      if (signal.aborted) abort();
    });
    setSnapshot((value) => ({
      ...(value.owner === owner ? value : empty(owner)),
      pending: phase,
      error: "",
    }));
    try {
      const accepted = await Promise.race([
        (async () => {
          if (phase !== "read") {
            if (!owner.attempt && phase === "choose" && reviewed && choice) {
              const context = await artistReviewApi.context(
                account,
                recording,
                { provider: reviewed.provider },
                signal,
              );
              signal.throwIfAborted();
              validateContext(context, reviewed);
              owner.attempt = {
                kind: "choose",
                body: Object.freeze({
                  request_id: requestId,
                  expected_revision: choice.revision,
                  association_id: reviewed.id,
                  context,
                }),
              };
            }
            if (!owner.attempt && phase === "reset" && choice)
              owner.attempt = {
                kind: "reset",
                body: Object.freeze({
                  request_id: requestId,
                  expected_revision: choice.revision,
                }),
              };
            const attempt = owner.attempt;
            if (!attempt)
              throw new Error("Artist choice request is unavailable");
            const body = attempt.body;
            const receipt =
              attempt.kind === "choose"
                ? await artistIdentityApi.choose(
                    account,
                    recording,
                    attempt.body,
                    signal,
                  )
                : await artistIdentityApi.reset(
                    account,
                    recording,
                    attempt.body,
                    signal,
                  );
            signal.throwIfAborted();
            if (
              !receipt ||
              receipt.request_id !== body.request_id ||
              receipt.recording_id !== recording ||
              receipt.revision !== body.expected_revision + 1 ||
              !Number.isSafeInteger(receipt.revision) ||
              (attempt.kind === "choose"
                ? receipt.selection?.association_id !==
                    attempt.body.association_id ||
                  typeof receipt.selection?.group_id !== "string" ||
                  !receipt.selection?.group_id
                : receipt.selection !== null)
            )
              throw new Error("Invalid artist identity receipt");
            owner.attempt = null;
            owner.choice = null;
          }
          const value = validateChoice(
            await artistIdentityApi.read(account, recording, signal),
          );
          signal.throwIfAborted();
          return value;
        })(),
        cancelled,
      ]);
      if (!active() || signal.aborted) return;
      owner.choice = accepted;
      reason = "accepted";
      setSnapshot({
        owner,
        choice: accepted,
        name: null,
        pending: phase,
        error: "",
        retrying: false,
      });
      if (accepted.state.status === "selected") {
        try {
          const groupId = accepted.state.group_id;
          const group = await Promise.race([
            artistIdentityApi.group(account, groupId, signal),
            cancelled,
          ]);
          if (
            active() &&
            !signal.aborted &&
            group?.group.id === groupId &&
            typeof group.group.name.value === "string"
          )
            setSnapshot((value) =>
              value.owner === owner
                ? { ...value, name: group.group.name.value }
                : value,
            );
        } catch {}
      }
    } catch (error) {
      if (active() && (!signal.aborted || expired)) {
        if (
          error instanceof HttpError &&
          error.status >= 400 &&
          error.status < 500
        ) {
          owner.attempt = null;
          owner.choice = null;
        }
        reason = expired ? "deadline" : "failed";
        setSnapshot((value) => ({
          ...(value.owner === owner ? value : empty(owner)),
          choice: owner.choice,
          name: null,
          retrying: owner.attempt !== null,
          error: owner.attempt
            ? "Hito could not confirm the artist choice. Retry the same request to recover its result."
            : "Hito could not complete the artist choice request. Check the current choice and try again.",
        }));
      }
    } finally {
      clearTimeout(timer);
      signal.removeEventListener("abort", abort);
      if (owner.controller === controller) {
        owner.controller = null;
        owner.phase = null;
        if (captured.active())
          setSnapshot((value) =>
            value.owner === owner ? { ...value, pending: null } : value,
          );
      }
      const record =
        reason === "failed" || reason === "deadline"
          ? log.warn
          : phase !== "read" && reason === "accepted"
            ? log.info
            : log.debug;
      record("artist_identity.finished", {
        action: phase,
        request_id: requestId,
        reason,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  useEffect(() => {
    if (opened) void perform("read");
    return () => owner.controller?.abort();
  }, [opened, owner, scope]);

  return {
    ...current,
    canChoose:
      !!current.choice &&
      !!association &&
      !current.retrying &&
      current.pending === null &&
      (current.choice.state.status !== "selected" ||
        current.choice.state.association_id !== association.id),
    choose: () => perform("choose", association),
    canReset:
      current.choice !== null &&
      current.choice.state.status !== "unselected" &&
      current.pending === null &&
      !current.retrying,
    reset: () => perform("reset"),
    retry: () =>
      owner.attempt ? perform(owner.attempt.kind) : Promise.resolve(),
    refresh: () => perform("read"),
  };
}
