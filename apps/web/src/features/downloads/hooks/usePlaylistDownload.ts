import { createUuid } from "../../../shared/uuid";
import { useEffect, useMemo, useState } from "react";
import { playlistDownloadApi } from "../api/playlistDownloadApi";
import { HttpError } from "../../../shared/http";
import { log } from "../../../shared/log";
import type { DownloadRequest, PlaylistDownloadStatus } from "../../../shared/types";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";

type Attempt =
  | { kind: "create"; body: Readonly<DownloadRequest> }
  | { kind: "remove"; retention: string };
type View = {
  download: PlaylistDownloadStatus | null;
  retention: string | null;
  checking: boolean;
  pending: boolean;
  attempt: Attempt | null;
  error: string;
  statusError: string;
};
type Owner = {
  view: View;
  progress: string | null;
  fastUntil: number;
  read: AbortController | null;
  write: AbortController | null;
  timer: ReturnType<typeof setTimeout> | null;
};

const fastWindowMs = 30_000;
const fastPollMs = 1000;
const slowPollMs = 30_000;

function busy(download: PlaylistDownloadStatus) {
  return (
    download.desired_revision !== download.reconciled_revision ||
    download.entries.items.some((entry) =>
      ["pending", "queued", "running", "verifying"].includes(entry.status),
    )
  );
}

function progressSignature(download: PlaylistDownloadStatus) {
  return JSON.stringify([
    download.retention_id,
    download.desired_revision,
    download.reconciled_revision,
    download.entries.next_cursor ?? null,
    download.entries.items.map((entry) => [
      entry.entry_id,
      entry.recording_id ?? null,
      entry.status,
      entry.source_id ?? null,
    ]),
  ]);
}

function hasFailedEntries(
  download: PlaylistDownloadStatus | null,
  retention: string | null,
) {
  return (
    download !== null &&
    download.retention_id === retention &&
    download.entries.items.some((entry) => entry.status === "failed")
  );
}

export function usePlaylistDownload(account: string, playlist: string) {
  const scope = useRequestScope(JSON.stringify([account, playlist]));
  const owner = useMemo<Owner>(
    () => ({
      view: {
        download: null,
        retention: null,
        checking: true,
        pending: false,
        attempt: null,
        error: "",
        statusError: "",
      },
      progress: null,
      fastUntil: 0,
      read: null,
      write: null,
      timer: null,
    }),
    [account, playlist],
  );
  const [state, setState] = useState<{ owner: Owner; view: View } | null>(null);
  const current = state?.owner === owner ? state.view : owner.view;
  const renderedRetention = current.retention;
  const renderedFailures = hasFailedEntries(
    current.download,
    renderedRetention,
  );

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
      (!owner.view.download && !owner.view.statusError)
    )
      return;
    owner.timer = setTimeout(
      () => void load(),
      !owner.view.statusError &&
        owner.view.download &&
        busy(owner.view.download) &&
        performance.now() + fastPollMs <= owner.fastUntil
        ? fastPollMs
        : slowPollMs,
    );
  }

  async function load(explicit = false): Promise<void> {
    const request = scope.capture();
    if (!request.active() || owner.read || owner.write || owner.view.attempt)
      return;
    clearTimer();
    if (explicit) owner.fastUntil = performance.now() + fastWindowMs;
    const controller = new AbortController();
    const signal = AbortSignal.any([request.signal, controller.signal]);
    const active = () =>
      request.active() && !signal.aborted && owner.read === controller;
    owner.read = controller;
    publish({ checking: true });
    const started = performance.now();
    const fields = { account_id: account, playlist_id: playlist };
    try {
      const download = await playlistDownloadApi.status(
        account,
        playlist,
        signal,
      );
      if (!active()) return;
      const progress = download ? progressSignature(download) : null;
      if (progress !== owner.progress)
        owner.fastUntil = performance.now() + fastWindowMs;
      owner.progress = progress;
      if (download === null) owner.fastUntil = 0;
      publish({
        download,
        retention: download?.retention_id ?? null,
        checking: false,
        statusError: "",
      });
      log.debug("playlist_download.status_checked", {
        ...fields,
        count: download?.entries.items.length ?? 0,
        mode: download ? "retained" : "absent",
        duration_ms: Math.round(performance.now() - started),
      });
    } catch (error) {
      if (active()) {
        publish({
          checking: false,
          statusError:
            "Hito could not check this playlist download. Try again.",
        });
        log.warn("playlist_download.status_failed", {
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
      if (!request.active() || signal.aborted)
        log.debug("playlist_download.status_cancelled", {
          ...fields,
          duration_ms: Math.round(performance.now() - started),
        });
    }
  }

  async function perform(attempt: Attempt): Promise<void> {
    const request = scope.capture();
    if (!request.active() || owner.write) return;
    clearTimer();
    owner.fastUntil = performance.now() + fastWindowMs;
    owner.read?.abort();
    owner.read = null;
    const controller = new AbortController();
    const signal = AbortSignal.any([request.signal, controller.signal]);
    const active = () =>
      request.active() && !signal.aborted && owner.write === controller;
    owner.write = controller;
    publish({
      attempt,
      pending: true,
      checking: false,
      error: "",
      statusError: "",
    });
    const started = performance.now();
    const fields = {
      account_id: account,
      playlist_id: playlist,
      operation_id:
        attempt.kind === "create" ? attempt.body.request_id : attempt.retention,
      action: attempt.kind,
    };
    const event = `playlist_download.${attempt.kind}`;
    let accepted = false;
    log.info(`${event}_started`, fields);
    try {
      if (attempt.kind === "create") {
        const receipt = await playlistDownloadApi.create(
          account,
          playlist,
          attempt.body,
          signal,
        );
        if (!active()) return;
        publish({ retention: receipt.retention_id });
      } else {
        await playlistDownloadApi.remove(account, attempt.retention, signal);
        if (!active()) return;
        if (owner.view.retention === attempt.retention)
          publish({ download: null, retention: null });
      }
      publish({ attempt: null });
      accepted = true;
      log.info(`${event}_accepted`, {
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
              ? "Hito could not start this playlist download. Check your access and try again."
              : "Hito could not remove this playlist download. Check your access and try again."
            : attempt.kind === "create"
              ? "Hito could not confirm the download request. Retry the same request to recover its result."
              : "Hito could not confirm removal. Retry removal to recover its result.",
        });
        log.warn(`${event}_failed`, {
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
          publish({ pending: false });
          if (accepted) void load();
          else schedule();
        }
      }
      if (!request.active() || signal.aborted)
        log.debug(`${event}_cancelled`, {
          ...fields,
          duration_ms: Math.round(performance.now() - started),
        });
    }
  }

  useEffect(() => {
    void load();
    const focused = () => void load(true);
    window.addEventListener("focus", focused);
    return () => {
      clearTimer();
      owner.read?.abort();
      owner.read = null;
      owner.write?.abort();
      owner.write = null;
      window.removeEventListener("focus", focused);
    };
  }, [account, playlist, owner, scope]);

  return {
    download: current.download,
    checking: current.checking,
    pending: current.pending,
    requested: current.retention !== null,
    hasRetainedFailures: renderedFailures,
    retryKind: current.attempt?.kind ?? null,
    error: current.error,
    statusError: current.statusError,
    check: () => load(true),
    start: () =>
      owner.view.attempt ||
      owner.view.retention ||
      owner.write ||
      owner.view.checking
        ? Promise.resolve()
        : perform({
            kind: "create",
            body: Object.freeze({ request_id: createUuid() }),
          }),
    remove: () =>
      owner.view.attempt ||
      !renderedRetention ||
      owner.view.retention !== renderedRetention ||
      owner.write ||
      owner.view.checking
        ? Promise.resolve()
        : perform({ kind: "remove", retention: renderedRetention }),
    retryFailed: () =>
      owner.view.attempt ||
      !renderedRetention ||
      owner.view.retention !== renderedRetention ||
      !renderedFailures ||
      !hasFailedEntries(owner.view.download, renderedRetention) ||
      owner.write ||
      owner.view.pending ||
      owner.view.checking
        ? Promise.resolve()
        : perform({
            kind: "create",
            body: Object.freeze({ request_id: createUuid() }),
          }),
    retry: () =>
      owner.view.attempt ? perform(owner.view.attempt) : Promise.resolve(),
  };
}
