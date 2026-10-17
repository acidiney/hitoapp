import { createUuid } from "../uuid";
import { useEffect, useMemo, useState } from "react";
import { downloadApi } from "../downloadApi";
import { HttpError } from "../http";
import { log } from "../log";
import type { DownloadRequest, DownloadStatus } from "../types";
import { useRequestScope } from "./useRequestScope";

type Attempt =
  | { kind: "create"; body: Readonly<DownloadRequest> }
  | { kind: "remove"; intent: string };
type View = {
  download: DownloadStatus | null;
  checking: boolean;
  pending: Attempt["kind"] | null;
  attempt: Attempt | null;
  error: string;
  statusError: string;
};
type Owner = {
  data: View;
  read: AbortController | null;
  write: AbortController | null;
  timer: ReturnType<typeof setTimeout> | null;
  invalidatedRead: boolean;
  choiceRevision: number | undefined;
};

const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const busy = (download: DownloadStatus | null) =>
  download !== null &&
  ["queued", "running", "verifying"].includes(download.status);

function validStatus(
  value: DownloadStatus,
  recording: string,
  choiceRevision: number | undefined,
) {
  return (
    value.recording_id === recording &&
    uuid.test(value.intent_id) &&
    typeof value.host_device_id === "string" &&
    value.host_device_id.length > 0 &&
    ["queued", "running", "verifying", "ready", "failed"].includes(
      value.status,
    ) &&
    Number.isSafeInteger(value.bytes_received) &&
    value.bytes_received >= 0 &&
    Number.isSafeInteger(value.choice_revision) &&
    value.choice_revision >= 0 &&
    (choiceRevision === undefined ||
      value.choice_revision === choiceRevision) &&
    typeof value.matches_source_choice === "boolean" &&
    (value.total_bytes == null ||
      (Number.isSafeInteger(value.total_bytes) &&
        value.total_bytes >= value.bytes_received)) &&
    (value.status !== "ready" ||
      (typeof value.source_id === "string" && uuid.test(value.source_id)))
  );
}

export function useRecordingDownload(account: string, recording: string) {
  const scope = useRequestScope(JSON.stringify([account, recording]));
  const owner = useMemo<Owner>(
    () => ({
      data: {
        download: null,
        checking: true,
        pending: null,
        attempt: null,
        error: "",
        statusError: "",
      },
      read: null,
      write: null,
      timer: null,
      invalidatedRead: false,
      choiceRevision: undefined,
    }),
    [account, recording],
  );
  const [state, setState] = useState<{ owner: Owner; data: View } | null>(null);
  const current = state?.owner === owner ? state.data : owner.data;

  function publish(update: Partial<View>) {
    owner.data = { ...owner.data, ...update };
    setState({ owner, data: owner.data });
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
      owner.data.attempt
    )
      return;
    if (!busy(owner.data.download) && !owner.data.statusError) return;
    owner.timer = setTimeout(
      () => void load(),
      owner.data.statusError ? 30_000 : 1000,
    );
  }

  async function load(): Promise<void> {
    const request = scope.capture();
    if (!request.active() || owner.read || owner.write || owner.data.attempt)
      return;
    clearTimer();
    const controller = new AbortController();
    const signal = AbortSignal.any([request.signal, controller.signal]);
    const active = () =>
      request.active() && !signal.aborted && owner.read === controller;
    owner.read = controller;
    owner.invalidatedRead = false;
    publish({ checking: true });
    const started = performance.now();
    const fields = { account_id: account, recording_id: recording };
    try {
      const download = await downloadApi.status(account, recording, signal);
      if (!active()) return;
      if (
        download !== null &&
        !validStatus(download, recording, owner.choiceRevision)
      )
        throw new Error("Hito returned an invalid download status");
      publish({ download, checking: false, statusError: "" });
      log.debug("download.status_checked", {
        ...fields,
        mode: download?.status ?? "absent",
        duration_ms: Math.round(performance.now() - started),
      });
    } catch (error) {
      if (active()) {
        publish({
          checking: false,
          statusError: "Hito could not check this download. Try again.",
        });
        log.warn("download.status_failed", {
          ...fields,
          reason: error instanceof HttpError ? "http" : "request",
          status: error instanceof HttpError ? error.status : undefined,
          duration_ms: Math.round(performance.now() - started),
        });
      }
    } finally {
      if (owner.read === controller) {
        owner.read = null;
        if (request.active() && !signal.aborted) schedule();
      }
    }
  }

  async function perform(attempt: Attempt): Promise<void> {
    const request = scope.capture();
    if (!request.active() || owner.write) return;
    clearTimer();
    owner.read?.abort();
    owner.read = null;
    const controller = new AbortController();
    const signal = AbortSignal.any([request.signal, controller.signal]);
    const active = () =>
      request.active() && !signal.aborted && owner.write === controller;
    owner.write = controller;
    publish({
      attempt,
      pending: attempt.kind,
      checking: owner.invalidatedRead,
      error: "",
      statusError: "",
    });
    const started = performance.now();
    const fields = {
      account_id: account,
      recording_id: recording,
      operation_id:
        attempt.kind === "create" ? attempt.body.request_id : undefined,
      action: attempt.kind,
    };
    let accepted = false;
    log.info("download.write_started", fields);
    try {
      if (attempt.kind === "create") {
        const receipt = await downloadApi.create(
          account,
          recording,
          attempt.body,
          signal,
        );
        if (!active()) return;
        if (
          receipt.request_id !== attempt.body.request_id ||
          receipt.recording_id !== recording ||
          !uuid.test(receipt.intent_id)
        )
          throw new Error("Hito returned a different download receipt");
      } else {
        await downloadApi.remove(account, attempt.intent, signal);
        if (!active()) return;
        if (owner.data.download?.intent_id === attempt.intent)
          publish({ download: null });
      }
      accepted = true;
      publish({ attempt: null });
      log.info("download.write_accepted", {
        ...fields,
        duration_ms: Math.round(performance.now() - started),
      });
    } catch (error) {
      if (active()) {
        const rejected =
          error instanceof HttpError &&
          error.status >= 400 &&
          error.status < 500;
        publish({
          attempt: rejected ? null : attempt,
          error: rejected
            ? attempt.kind === "create"
              ? "Hito could not start this download. Check audio availability and try again."
              : "Hito could not remove this download. Check your access and try again."
            : attempt.kind === "create"
              ? "Hito could not confirm the download request. Retry the same request to recover its result."
              : "Hito could not confirm removal. Retry removal to recover its result.",
        });
        log.warn("download.write_failed", {
          ...fields,
          reason: rejected ? "rejected" : "unconfirmed",
          status: error instanceof HttpError ? error.status : undefined,
          duration_ms: Math.round(performance.now() - started),
        });
      }
    } finally {
      if (owner.write === controller) {
        owner.write = null;
        if (request.active() && !signal.aborted) {
          publish({ pending: null });
          if (accepted || (owner.invalidatedRead && !owner.data.attempt))
            void load();
          else schedule();
        }
      }
    }
  }

  function invalidateRead(revision: number) {
    if (!scope.capture().active()) return;
    const initial = owner.choiceRevision === undefined;
    owner.choiceRevision = revision;
    if (
      initial &&
      (owner.read !== null ||
        owner.data.download === null ||
        owner.data.download.choice_revision === revision)
    )
      return;
    clearTimer();
    owner.read?.abort();
    owner.read = null;
    owner.invalidatedRead = true;
    publish({ checking: true, statusError: "" });
    if (!owner.write && !owner.data.attempt) void load();
  }

  useEffect(() => {
    void load();
    const focused = () => void load();
    window.addEventListener("focus", focused);
    return () => {
      clearTimer();
      owner.read?.abort();
      owner.read = null;
      owner.write?.abort();
      owner.write = null;
      window.removeEventListener("focus", focused);
    };
  }, [owner, scope, account, recording]);

  return {
    download: current.download,
    checking: current.checking,
    pending: current.pending,
    retryKind: current.attempt?.kind ?? null,
    error: current.error,
    statusError: current.statusError,
    check: load,
    invalidateRead,
    start: () =>
      owner.data.attempt ||
      owner.data.checking ||
      owner.data.statusError ||
      owner.data.download?.matches_source_choice ||
      owner.write
        ? Promise.resolve()
        : perform({
            kind: "create",
            body: Object.freeze({ request_id: createUuid() }),
          }),
    remove: () =>
      owner.data.attempt ||
      owner.data.checking ||
      owner.data.statusError ||
      !owner.data.download ||
      owner.write
        ? Promise.resolve()
        : perform({ kind: "remove", intent: owner.data.download.intent_id }),
    retry: () =>
      owner.data.attempt ? perform(owner.data.attempt) : Promise.resolve(),
  };
}
