import type { Schema } from "../../../shared/schema";
import { createUuid } from "../../../shared/uuid";
import { useEffect, useMemo, useRef, useState } from "react";
import { HttpError } from "../../../shared/http";
import { log } from "../../../shared/log";
import { msUntilCheck } from "../api/preparationPolicy";
import { sourcePreparationApi } from "../api/sourcePreparationApi";

import { useRequestScope } from "../../../shared/hooks/useRequestScope";

type View = {
  status: Schema<"SourcePreparationStatus"> | null;
  receipt: Schema<"SourcePreparationReceipt"> | null;
  attempt: Readonly<Schema<"PrepareSource">> | null;
  checking: boolean;
  pending: boolean;
  halted: boolean;
  error: string;
  statusError: string;
};
type Owner = {
  view: View;
  read: AbortController | null;
  write: AbortController | null;
  timer: ReturnType<typeof setTimeout> | null;
  signature: string;
  changedAt: number;
  failures: number;
  confirmed: string | null;
};
type ConfirmReady = (source: string, signal: AbortSignal) => Promise<boolean>;

function polling(status: Schema<"SourcePreparationStatus"> | null) {
  return (
    status !== null &&
    (["queued", "submitting", "preparing"].includes(status.phase) ||
      (["blocked", "submission_unknown"].includes(status.phase) &&
        status.next_check_at != null))
  );
}

export function useSourcePreparation(
  account: string,
  recording: string,
  source: Schema<"Source"> | null,
  confirmReady: ConfirmReady,
  refreshRevision = 0,
) {
  const sourceId = source?.id ?? null;
  const scope = useRequestScope(JSON.stringify([account, recording, sourceId]));
  const confirm = useRef(confirmReady);
  confirm.current = confirmReady;
  const owner = useMemo<Owner>(
    () => ({
      view: {
        status: null,
        receipt: null,
        attempt: null,
        checking: sourceId !== null,
        pending: false,
        halted: false,
        error: "",
        statusError: "",
      },
      read: null,
      write: null,
      timer: null,
      signature: "",
      changedAt: performance.now(),
      failures: 0,
      confirmed: null,
    }),
    [scope, sourceId],
  );
  const [state, setState] = useState<{ owner: Owner; view: View } | null>(null);
  const current = state?.owner === owner ? state.view : owner.view;
  const fields = {
    account_id: account,
    recording_id: recording,
    source_id: sourceId ?? undefined,
  };

  function publish(update: Partial<View>) {
    owner.view = { ...owner.view, ...update };
    setState({ owner, view: owner.view });
  }

  function clearTimer() {
    if (owner.timer !== null) clearTimeout(owner.timer);
    owner.timer = null;
  }

  function schedule() {
    clearTimer();
    if (
      !scope.capture().active() ||
      owner.read ||
      owner.write ||
      owner.view.attempt ||
      owner.view.halted ||
      (!owner.failures && !polling(owner.view.status))
    )
      return;
    const backoff = [5_000, 15_000, 60_000, 300_000];
    const interval = owner.failures
      ? backoff[Math.min(owner.failures - 1, backoff.length - 1)]
      : performance.now() - owner.changedAt < 30_000
        ? 5_000
        : 30_000;
    owner.timer = setTimeout(
      () => void load(),
      Math.min(86_400_000, Math.max(interval, msUntilCheck(owner.view.status))),
    );
  }

  async function load(): Promise<void> {
    const request = scope.capture();
    if (
      sourceId === null ||
      !request.active() ||
      owner.read ||
      owner.write ||
      owner.view.attempt ||
      owner.view.halted
    )
      return;
    clearTimer();
    const controller = new AbortController();
    const signal = AbortSignal.any([request.signal, controller.signal]);
    const active = () =>
      request.active() && !signal.aborted && owner.read === controller;
    owner.read = controller;
    publish({ checking: true });
    const started = performance.now();
    try {
      const status = await sourcePreparationApi.status(
        account,
        recording,
        sourceId,
        signal,
      );
      if (!active()) return;
      const pinned =
        owner.view.receipt?.preparation_id ?? owner.view.status?.preparation_id;
      if (pinned && status?.preparation_id !== pinned) {
        publish({
          status: null,
          halted: true,
          statusError: status
            ? "This source has a different preparation. Check audio availability again."
            : "This source preparation is no longer available. Check audio availability again.",
        });
        return;
      }
      const signature = status
        ? JSON.stringify([
            status.phase,
            status.progress_basis_points,
            status.eta_seconds,
            status.reason,
          ])
        : "absent";
      if (signature !== owner.signature) owner.changedAt = performance.now();
      owner.signature = signature;
      owner.failures = 0;
      publish({ status, statusError: "" });
      log.debug("source_preparation.status_checked", {
        ...fields,
        preparation_id: status?.preparation_id,
        mode: status?.phase ?? "absent",
        duration_ms: Math.round(performance.now() - started),
      });
      if (
        status?.phase === "ready" &&
        owner.confirmed !== status.preparation_id
      ) {
        owner.confirmed = status.preparation_id;
        const ready = await confirm.current(sourceId, signal);
        if (!active()) return;
        if (!ready)
          publish({
            statusError:
              "Hito could not confirm playable audio. Check availability again.",
          });
      }
    } catch (error) {
      if (active()) {
        const denied =
          error instanceof HttpError && [401, 403, 404].includes(error.status);
        owner.failures =
          denied || owner.view.status?.phase === "ready"
            ? 0
            : owner.failures + 1;
        publish({
          halted: denied,
          status: denied ? null : owner.view.status,
          statusError: denied
            ? "This source preparation is no longer available. Check audio availability again."
            : owner.view.status?.phase === "ready"
              ? "Hito could not confirm playable audio. Check availability again."
              : "Hito could not check preparation. Try again.",
        });
        log.warn("source_preparation.status_failed", {
          ...fields,
          reason: denied ? "authority" : "request",
          status: error instanceof HttpError ? error.status : undefined,
          duration_ms: Math.round(performance.now() - started),
        });
      }
    } finally {
      if (owner.read === controller) {
        owner.read = null;
        if (request.active() && !signal.aborted) {
          publish({ checking: false });
          schedule();
        }
      }
    }
  }

  async function perform(
    body: Readonly<Schema<"PrepareSource">>,
  ): Promise<void> {
    const request = scope.capture();
    if (sourceId === null || !request.active() || owner.write) return;
    clearTimer();
    owner.read?.abort();
    owner.read = null;
    const controller = new AbortController();
    const signal = AbortSignal.any([request.signal, controller.signal]);
    const active = () =>
      request.active() && !signal.aborted && owner.write === controller;
    owner.write = controller;
    publish({
      attempt: body,
      pending: true,
      checking: false,
      error: "",
      statusError: "",
    });
    const started = performance.now();
    const writeFields = { ...fields, operation_id: body.request_id };
    let accepted = false;
    log.info("source_preparation.write_started", writeFields);
    try {
      const receipt = await sourcePreparationApi.create(
        account,
        recording,
        sourceId,
        body,
        signal,
      );
      if (!active()) return;
      publish({ receipt, attempt: null, status: null, halted: false });
      owner.failures = 0;
      accepted = true;
      log.info("source_preparation.write_accepted", {
        ...writeFields,
        preparation_id: receipt.preparation_id,
        duration_ms: Math.round(performance.now() - started),
      });
    } catch (error) {
      if (active()) {
        const rejected =
          error instanceof HttpError &&
          error.status >= 400 &&
          error.status < 500 &&
          error.status !== 408;
        publish({
          attempt: rejected ? null : body,
          error: rejected
            ? "Hito could not start preparation. Check audio availability and try again."
            : "Hito could not confirm the preparation request. Retry the same request to recover its result.",
        });
        log.warn("source_preparation.write_failed", {
          ...writeFields,
          reason: rejected ? "rejected" : "unconfirmed",
          status: error instanceof HttpError ? error.status : undefined,
          duration_ms: Math.round(performance.now() - started),
        });
      }
    } finally {
      if (owner.write === controller) {
        owner.write = null;
        if (request.active() && !signal.aborted) {
          publish({ pending: false });
          if (accepted) void load();
        }
      }
    }
  }

  useEffect(() => {
    if (sourceId !== null) void load();
    return () => {
      clearTimer();
      owner.read?.abort();
      owner.read = null;
      owner.write?.abort();
      owner.write = null;
    };
  }, [owner, scope, account, recording, sourceId]);

  useEffect(() => {
    if (!owner.view.halted || owner.view.attempt || owner.write) return;
    owner.failures = 0;
    owner.confirmed = null;
    owner.signature = "";
    publish({
      status: null,
      receipt: null,
      halted: false,
      statusError: "",
      error: "",
    });
    void load();
  }, [owner, refreshRevision]);

  function startable(view: View) {
    return (
      source?.can_prepare === true &&
      source.ready === false &&
      !view.attempt &&
      !view.halted &&
      (view.status?.phase === "failed" ||
        view.status?.phase === "ready" ||
        (!view.status && !view.receipt))
    );
  }

  return {
    ...current,
    canStart: startable(current),
    check: load,
    start: () =>
      owner.write || !startable(owner.view)
        ? Promise.resolve()
        : perform(Object.freeze({ request_id: createUuid() })),
    retry: () =>
      owner.view.attempt ? perform(owner.view.attempt) : Promise.resolve(),
  };
}
