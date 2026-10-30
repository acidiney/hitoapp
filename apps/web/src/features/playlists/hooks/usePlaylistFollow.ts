import type { Schema } from "../../../shared/schema";
import { createUuid } from "../../../shared/uuid";
import { useEffect, useMemo, useState } from "react";
import { playlistFollowApi } from "../api/playlistFollowApi";
import { HttpError } from "../../../shared/http";
import { log } from "../../../shared/log";

import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { useFollowDetail } from "./useFollowDetail";

type RequestRun = { controller: AbortController };
type FollowOwner = {
  follow: Schema<"PlaylistFollow"> | null;
  read: RequestRun | null;
  write: RequestRun | null;
  timer: ReturnType<typeof setTimeout> | null;
  attempt: Readonly<Schema<"RefreshFollowedPlaylist">> | null;
  waiting: boolean;
  statusError: string;
  refreshError: string;
};
type FollowState = {
  owner: FollowOwner;
  follow: Schema<"PlaylistFollow"> | null;
  loading: boolean;
  pending: boolean;
  hasAttempt: boolean;
  statusError: string;
  refreshError: string;
};

function busy(follow: Schema<"PlaylistFollow"> | null) {
  return follow?.status === "queued" || follow?.status === "running";
}

export function usePlaylistFollow(
  account: string,
  playlist: string | null,
  renderedRevision: number | null,
) {
  const scope = useRequestScope(JSON.stringify([account, playlist]));
  const owner = useMemo<FollowOwner>(
    () => ({
      follow: null,
      read: null,
      write: null,
      timer: null,
      attempt: null,
      waiting: false,
      statusError: "",
      refreshError: "",
    }),
    [account, playlist],
  );
  const detail = useFollowDetail(account, playlist, renderedRevision);
  const [state, setState] = useState<FollowState | null>(null);
  const current = state?.owner === owner ? state : null;

  function publish(loading = false) {
    setState({
      owner,
      follow: owner.follow,
      loading,
      pending: owner.write !== null || owner.waiting,
      hasAttempt: owner.attempt !== null,
      statusError: owner.statusError,
      refreshError: owner.refreshError,
    });
  }

  function clearTimer() {
    if (owner.timer !== null) clearTimeout(owner.timer);
    owner.timer = null;
  }

  function schedule() {
    clearTimer();
    if (!playlist || !scope.capture().active() || owner.read || owner.write)
      return;
    owner.timer = setTimeout(
      () => void load(),
      owner.statusError ? 30_000 : busy(owner.follow) ? 1000 : 30_000,
    );
  }

  async function load(): Promise<void> {
    const request = scope.capture();
    if (!playlist || !request.active() || owner.read || owner.write) return;
    clearTimer();
    const run = { controller: new AbortController() };
    const signal = AbortSignal.any([request.signal, run.controller.signal]);
    const active = () =>
      request.active() && !signal.aborted && owner.read === run;
    owner.read = run;
    publish(owner.follow === null);
    const started = performance.now();
    const fields = { account_id: account, playlist_id: playlist };
    log.debug("playlist_follow.status_started", fields);
    try {
      const follow = await playlistFollowApi.status(account, playlist, signal);
      if (!active()) return;
      if (follow.playlist_id !== playlist)
        throw new Error("Hito returned a different playlist status");
      const completed = owner.waiting && !busy(follow);
      const reload =
        !busy(follow) &&
        (completed || detail.differsFromRendered(follow.content_revision));
      owner.follow = follow;
      owner.waiting = busy(follow);
      owner.statusError = "";
      publish();
      log.debug("playlist_follow.status_checked", {
        ...fields,
        mode: follow.status,
        duration_ms: Math.round(performance.now() - started),
      });
      if (reload) {
        detail.markChanged(follow.content_revision, completed);
      }
      if (!busy(follow)) detail.reload();
    } catch (error) {
      if (active()) {
        owner.statusError = "Hito could not check this playlist. Try again.";
        publish();
        log.warn("playlist_follow.status_failed", {
          ...fields,
          reason: error instanceof HttpError ? "http" : "request",
          status: error instanceof HttpError ? error.status : undefined,
          duration_ms: Math.round(performance.now() - started),
        });
      }
    } finally {
      if (owner.read === run) {
        owner.read = null;
        if (request.active() && !signal.aborted) schedule();
      }
      if (!request.active() || signal.aborted)
        log.debug("playlist_follow.status_cancelled", fields);
    }
  }

  async function perform(
    body: Readonly<Schema<"RefreshFollowedPlaylist">>,
    action: "refresh" | "retry",
  ): Promise<boolean> {
    const request = scope.capture();
    if (
      !playlist ||
      !request.active() ||
      owner.write ||
      owner.follow?.status === "paused"
    )
      return false;
    clearTimer();
    owner.read?.controller.abort();
    owner.read = null;
    const run = { controller: new AbortController() };
    const signal = AbortSignal.any([request.signal, run.controller.signal]);
    const active = () =>
      request.active() && !signal.aborted && owner.write === run;
    owner.write = run;
    owner.attempt = body;
    owner.statusError = "";
    owner.refreshError = "";
    publish();
    const started = performance.now();
    const fields = {
      account_id: account,
      playlist_id: playlist,
      operation_id: body.request_id,
      action,
    };
    let accepted = false;
    log.info("playlist_follow.refresh_started", fields);
    try {
      const receipt = await playlistFollowApi.refresh(
        account,
        playlist,
        body,
        signal,
      );
      if (!active()) return false;
      if (
        receipt.request_id !== body.request_id ||
        receipt.playlist_id !== playlist
      )
        throw new Error("Hito returned a different playlist refresh receipt");
      owner.attempt = null;
      owner.waiting = true;
      accepted = true;
      log.info("playlist_follow.refresh_queued", {
        ...fields,
        job_id: receipt.job_id,
        duration_ms: Math.round(performance.now() - started),
      });
      return true;
    } catch (error) {
      if (active()) {
        owner.refreshError =
          "Hito could not confirm the refresh request. Retry the same request to recover its result.";
        log.warn("playlist_follow.refresh_failed", {
          ...fields,
          reason: error instanceof HttpError ? "http" : "request",
          status: error instanceof HttpError ? error.status : undefined,
          duration_ms: Math.round(performance.now() - started),
        });
      }
      return false;
    } finally {
      if (owner.write === run) {
        owner.write = null;
        if (request.active() && !signal.aborted) {
          publish();
          if (accepted) void load();
          else schedule();
        }
      }
      if (!request.active() || signal.aborted)
        log.debug("playlist_follow.refresh_cancelled", fields);
    }
  }

  useEffect(() => {
    if (!playlist) return;
    void load();
    const focused = () => void load();
    window.addEventListener("focus", focused);
    return () => {
      clearTimer();
      owner.read?.controller.abort();
      owner.read = null;
      owner.write?.controller.abort();
      owner.write = null;
      window.removeEventListener("focus", focused);
    };
  }, [account, playlist, owner, scope]);

  return {
    follow: current?.follow ?? null,
    loading: playlist !== null && (current?.loading ?? true),
    pending: current?.pending ?? false,
    hasAttempt: current?.hasAttempt ?? false,
    statusError: current?.statusError ?? "",
    refreshError: current?.refreshError ?? "",
    detailError: detail.detailError,
    refresh: () =>
      owner.attempt || owner.waiting || !owner.follow
        ? Promise.resolve(false)
        : perform({ request_id: createUuid() }, "refresh"),
    retryRefresh: () =>
      owner.attempt ? perform(owner.attempt, "retry") : Promise.resolve(false),
    retryStatus: load,
  };
}
