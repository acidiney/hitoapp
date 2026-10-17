import { createUuid } from "../uuid";
import { useEffect, useMemo, useRef, useState } from "react";
import { HttpError } from "../http";
import { log } from "../log";
import { sourceChoiceApi } from "../sourceChoiceApi";
import type { ChooseSource, ResetSourceChoice, SourceChoice } from "../types";
import { useRequestScope } from "./useRequestScope";

type Attempt =
  | { kind: "choose"; body: Readonly<ChooseSource> }
  | { kind: "reset"; body: Readonly<ResetSourceChoice> };
type View = {
  choice: SourceChoice | null;
  stagedSource: string | null;
  checking: boolean;
  pending: Attempt["kind"] | null;
  attempt: Attempt | null;
  needsCheck: boolean;
  unavailable: boolean;
  error: string;
  statusError: string;
};
type Owner = {
  view: View;
  read: AbortController | null;
  write: AbortController | null;
  refreshDownload: boolean;
};

export function useSourceChoice(
  account: string,
  recording: string,
  confirmed: (revision: number) => void,
) {
  const scope = useRequestScope(JSON.stringify([account, recording]));
  const onConfirmed = useRef(confirmed);
  onConfirmed.current = confirmed;
  const owner = useMemo<Owner>(
    () => ({
      view: {
        choice: null,
        stagedSource: null,
        checking: true,
        pending: null,
        attempt: null,
        needsCheck: false,
        unavailable: false,
        error: "",
        statusError: "",
      },
      read: null,
      write: null,
      refreshDownload: false,
    }),
    [account, recording],
  );
  const [state, setState] = useState<{ owner: Owner; view: View } | null>(null);
  const current = state?.owner === owner ? state.view : owner.view;

  function publish(update: Partial<View>) {
    owner.view = { ...owner.view, ...update };
    setState({ owner, view: owner.view });
  }

  function writable(view: View) {
    return (
      view.choice !== null &&
      !view.checking &&
      !view.pending &&
      !view.attempt &&
      !view.needsCheck &&
      !view.statusError &&
      !view.unavailable
    );
  }

  async function load(): Promise<void> {
    const request = scope.capture();
    if (!request.active() || owner.read || owner.write || owner.view.attempt)
      return;
    const controller = new AbortController();
    const signal = AbortSignal.any([request.signal, controller.signal]);
    const active = () =>
      request.active() && !signal.aborted && owner.read === controller;
    owner.read = controller;
    publish({ checking: true, statusError: "" });
    try {
      const choice = await sourceChoiceApi.read(account, recording, signal);
      if (!active()) return;
      if (choice === null && owner.view.needsCheck) {
        publish({
          statusError:
            "Hito could not check the current source choice. Try again.",
        });
        return;
      }
      const previousRevision = owner.view.choice?.revision;
      publish({
        choice,
        stagedSource: null,
        needsCheck: false,
        unavailable: choice === null,
        error: "",
        statusError: "",
      });
      if (
        choice !== null &&
        (owner.refreshDownload || choice.revision !== previousRevision)
      ) {
        owner.refreshDownload = false;
        onConfirmed.current(choice.revision);
      }
    } catch {
      if (active())
        publish({
          statusError:
            "Hito could not check the current source choice. Try again.",
        });
    } finally {
      if (owner.read === controller) {
        owner.read = null;
        if (request.active() && !signal.aborted) publish({ checking: false });
      }
    }
  }

  async function perform(attempt: Attempt): Promise<void> {
    const request = scope.capture();
    if (!request.active() || owner.write || owner.read) return;
    const controller = new AbortController();
    const signal = AbortSignal.any([request.signal, controller.signal]);
    const active = () =>
      request.active() && !signal.aborted && owner.write === controller;
    owner.write = controller;
    publish({ attempt, pending: attempt.kind, error: "", statusError: "" });
    const started = performance.now();
    const fields = {
      account_id: account,
      recording_id: recording,
      operation_id: attempt.body.request_id,
      source_id: attempt.kind === "choose" ? attempt.body.source_id : undefined,
      action: attempt.kind,
    };
    let recheck = false;
    log.info("source_choice.write_started", fields);
    try {
      if (attempt.kind === "choose")
        await sourceChoiceApi.choose(account, recording, attempt.body, signal);
      else
        await sourceChoiceApi.reset(account, recording, attempt.body, signal);
      if (!active()) return;
      owner.refreshDownload = true;
      recheck = true;
      publish({ attempt: null, needsCheck: true });
      log.info("source_choice.write_accepted", {
        ...fields,
        duration_ms: Math.round(performance.now() - started),
      });
    } catch (error) {
      if (active()) {
        const rejected =
          error instanceof HttpError &&
          error.status >= 400 &&
          error.status < 500;
        recheck = rejected;
        publish({
          attempt: rejected ? null : attempt,
          needsCheck: rejected || owner.view.needsCheck,
          error: rejected
            ? "Hito could not change the source choice. Check the current choice and try again."
            : "Hito could not confirm the source choice. Retry the same request to recover its result.",
        });
        log.warn("source_choice.write_failed", {
          ...fields,
          reason: rejected ? "rejected" : "unconfirmed",
          status: error instanceof HttpError ? error.status : undefined,
          duration_ms: Math.round(performance.now() - started),
        });
      }
    } finally {
      if (signal.aborted)
        log.debug("source_choice.write_cancelled", {
          ...fields,
          duration_ms: Math.round(performance.now() - started),
        });
      if (owner.write === controller) {
        owner.write = null;
        if (request.active() && !signal.aborted) {
          publish({ pending: null });
          if (recheck) void load();
        }
      }
    }
  }

  function invalidateRead() {
    if (!scope.capture().active() || owner.view.unavailable) return;
    if (owner.view.choice !== null) publish({ needsCheck: true });
    if (owner.write || owner.view.attempt) return;
    owner.read?.abort();
    owner.read = null;
    void load();
  }

  useEffect(() => {
    void load();
    return () => {
      owner.read?.abort();
      owner.read = null;
      owner.write?.abort();
      owner.write = null;
    };
  }, [owner, scope, account, recording]);

  const selectable = (view: View, source: string | null) =>
    source !== null &&
    view.choice?.sources.some(
      (entry) => entry.source.id === source && entry.selectable,
    ) === true;

  return {
    choice: current.choice,
    stagedSource: current.stagedSource,
    checking: current.checking,
    pending: current.pending !== null,
    retrying: current.attempt !== null,
    unavailable: current.unavailable,
    error: current.error,
    statusError: current.statusError,
    eligibilityPending:
      current.pending !== null ||
      current.attempt !== null ||
      current.needsCheck,
    blocked: !writable(current),
    canChoose: writable(current) && selectable(current, current.stagedSource),
    canReset: writable(current) && current.choice?.mode !== "automatic",
    stage: (source: string) => {
      if (
        scope.capture().active() &&
        writable(owner.view) &&
        selectable(owner.view, source)
      )
        publish({ stagedSource: source, error: "" });
    },
    choose: () => {
      const source = owner.view.stagedSource;
      return writable(owner.view) && selectable(owner.view, source) && source
        ? perform({
            kind: "choose",
            body: Object.freeze({
              request_id: createUuid(),
              source_id: source,
            }),
          })
        : Promise.resolve();
    },
    reset: () => {
      const choice = owner.view.choice;
      return writable(owner.view) && choice && choice.mode !== "automatic"
        ? perform({
            kind: "reset",
            body: Object.freeze({
              request_id: createUuid(),
              expected_revision: choice.revision,
            }),
          })
        : Promise.resolve();
    },
    retry: () =>
      owner.view.attempt ? perform(owner.view.attempt) : Promise.resolve(),
    check: load,
    invalidateRead,
  };
}
