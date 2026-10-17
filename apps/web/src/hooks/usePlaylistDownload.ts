import { useEffect, useMemo, useState } from "react";
import { playlistDownloadApi } from "../playlistDownloadApi";
import { HttpError } from "../http";
import { log } from "../log";
import type { DownloadRequest, PlaylistDownloadStatus } from "../types";
import { useRequestScope } from "./useRequestScope";

type View = {
  download: PlaylistDownloadStatus | null;
  retention: string | null;
  checking: boolean;
  pending: boolean;
  attempt: Readonly<DownloadRequest> | null;
  error: string;
  statusError: string;
};
type Owner = {
  view: View;
  read: AbortController | null;
  write: AbortController | null;
  timer: ReturnType<typeof setTimeout> | null;
};

function busy(download: PlaylistDownloadStatus) {
  return (
    download.desired_revision !== download.reconciled_revision ||
    download.entries.items.some((entry) =>
      ["pending", "queued", "running", "verifying"].includes(entry.status),
    )
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
      read: null,
      write: null,
      timer: null,
    }),
    [account, playlist],
  );
  const [state, setState] = useState<{ owner: Owner; view: View } | null>(null);
  const current = state?.owner === owner ? state.view : owner.view;

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
        busy(owner.view.download)
        ? 1000
        : 30_000,
    );
  }

  async function load(): Promise<void> {
    const request = scope.capture();
    if (!request.active() || owner.read || owner.write || owner.view.attempt)
      return;
    clearTimer();
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

  async function perform(body: Readonly<DownloadRequest>): Promise<void> {
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
      attempt: body,
      pending: true,
      checking: false,
      error: "",
      statusError: "",
    });
    const started = performance.now();
    const fields = {
      account_id: account,
      playlist_id: playlist,
      operation_id: body.request_id,
    };
    let accepted = false;
    log.info("playlist_download.create_started", fields);
    try {
      const receipt = await playlistDownloadApi.create(
        account,
        playlist,
        body,
        signal,
      );
      if (!active()) return;
      publish({ attempt: null, retention: receipt.retention_id });
      accepted = true;
      log.info("playlist_download.create_accepted", {
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
          attempt: rejected ? null : body,
          error: rejected
            ? "Hito could not start this playlist download. Check your access and try again."
            : "Hito could not confirm the download request. Retry the same request to recover its result.",
        });
        log.warn("playlist_download.create_failed", {
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
        log.debug("playlist_download.create_cancelled", {
          ...fields,
          duration_ms: Math.round(performance.now() - started),
        });
    }
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
  }, [account, playlist, owner, scope]);

  return {
    download: current.download,
    checking: current.checking,
    pending: current.pending,
    requested: current.retention !== null,
    hasAttempt: current.attempt !== null,
    error: current.error,
    statusError: current.statusError,
    check: load,
    start: () =>
      owner.view.attempt ||
      owner.view.retention ||
      owner.write ||
      owner.view.checking
        ? Promise.resolve()
        : perform(Object.freeze({ request_id: crypto.randomUUID() })),
    retry: () =>
      owner.view.attempt ? perform(owner.view.attempt) : Promise.resolve(),
  };
}
