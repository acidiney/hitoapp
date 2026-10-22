import { useEffect, useMemo, useRef, useState } from "react";
import { prepareChange, submitChange } from "../../../shared/sync";
import { pluginsApi } from "../../../pluginsApi";
import { playlistFollowApi } from "../api/playlistFollowApi";
import { HttpError } from "../../../shared/http";
import { log } from "../../../shared/log";
import type {
  PlaylistDetail,
  PluginInstallation,
  PluginService,
  SyncBatch,
} from "../../../shared/types";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";

export type FollowResumeReview = {
  installation: PluginInstallation;
  service: PluginService;
  generation: number;
};
type ResumeOwner = {
  run: AbortController | null;
  review: FollowResumeReview | null;
  attempt: SyncBatch | null;
  error: string;
};
type ResumeState = {
  owner: ResumeOwner;
  review: FollowResumeReview | null;
  pending: boolean;
  hasAttempt: boolean;
  error: string;
};

function positiveRevision(value: number) {
  return Number.isSafeInteger(value) && value > 0;
}

export function useFollowResume(
  account: string,
  playlist: string | null,
  origin: PlaylistDetail["origin"],
  recheckStatus: () => Promise<void>,
) {
  const key = JSON.stringify([
    account,
    playlist,
    origin?.installation_id,
    origin?.service_id,
    origin?.provider,
  ]);
  const scope = useRequestScope(key);
  const owner = useMemo<ResumeOwner>(
    () => ({ run: null, review: null, attempt: null, error: "" }),
    [key],
  );
  const recheck = useRef(recheckStatus);
  recheck.current = recheckStatus;
  const [state, setState] = useState<ResumeState | null>(null);
  const current = state?.owner === owner ? state : null;

  function publish() {
    setState({
      owner,
      review: owner.review,
      pending: owner.run !== null,
      hasAttempt: owner.attempt !== null,
      error: owner.error,
    });
  }

  useEffect(
    () => () => {
      owner.run?.abort();
      owner.run = null;
    },
    [owner],
  );

  async function reviewConnection(): Promise<boolean> {
    const request = scope.capture();
    if (!playlist || !origin || !request.active() || owner.run || owner.attempt)
      return false;
    const run = new AbortController();
    const signal = AbortSignal.any([request.signal, run.signal]);
    const active = () =>
      request.active() && !signal.aborted && owner.run === run;
    owner.run = run;
    owner.review = null;
    owner.error = "";
    publish();
    const started = performance.now();
    const fields = { account_id: account, playlist_id: playlist };
    log.info("playlist_follow.review_started", fields);
    try {
      const [settings, follow] = await Promise.all([
        pluginsApi.settings(account, signal),
        playlistFollowApi.status(account, playlist, signal),
      ]);
      if (!active()) return false;
      const installation = settings.installations.find(
        (value) => value.id === origin.installation_id,
      );
      const service = settings.services.find(
        (value) => value.id === origin.service_id,
      );
      if (
        !installation ||
        !service ||
        installation.service_id !== origin.service_id ||
        installation.manifest.id !== origin.provider ||
        !installation.manifest.capabilities.includes("playlists") ||
        !positiveRevision(installation.revision) ||
        !positiveRevision(service.revision) ||
        follow.playlist_id !== playlist ||
        !positiveRevision(follow.generation) ||
        follow.status !== "paused"
      ) {
        owner.error =
          "This playlist's original connection cannot be approved for updates. Check plugin settings and the latest playlist status, then review again.";
        log.warn("playlist_follow.review_rejected", {
          ...fields,
          reason: "connection_changed",
          duration_ms: Math.round(performance.now() - started),
        });
        return false;
      }
      owner.review = { installation, service, generation: follow.generation };
      log.info("playlist_follow.review_ready", {
        ...fields,
        duration_ms: Math.round(performance.now() - started),
      });
      return true;
    } catch (error) {
      if (active()) {
        owner.error =
          "Hito could not review this connection. Check plugin settings and try again.";
        log.warn("playlist_follow.review_failed", {
          ...fields,
          reason: error instanceof HttpError ? "http" : "request",
          status: error instanceof HttpError ? error.status : undefined,
          duration_ms: Math.round(performance.now() - started),
        });
      }
      return false;
    } finally {
      run.abort();
      if (owner.run === run) {
        owner.run = null;
        if (request.active()) publish();
      }
      if (!request.active())
        log.debug("playlist_follow.review_cancelled", fields);
    }
  }

  async function perform(batch: SyncBatch, retry: boolean): Promise<boolean> {
    const request = scope.capture();
    if (!playlist || !origin || !request.active() || owner.run) return false;
    const run = new AbortController();
    const signal = AbortSignal.any([request.signal, run.signal]);
    const active = () =>
      request.active() && !signal.aborted && owner.run === run;
    owner.run = run;
    owner.attempt = batch;
    owner.error = "";
    publish();
    const started = performance.now();
    const fields = {
      account_id: account,
      playlist_id: playlist,
      operation_id: batch.operations[0].id,
      action: retry ? "retry" : "resume",
    };
    let accepted = false;
    log.info("playlist_follow.resume_started", fields);
    try {
      await submitChange(account, batch, signal);
      if (!active()) return false;
      owner.attempt = null;
      owner.review = null;
      accepted = true;
      log.info("playlist_follow.resume_accepted", {
        ...fields,
        duration_ms: Math.round(performance.now() - started),
      });
    } catch (error) {
      if (active()) {
        if (error instanceof HttpError && error.status === 409) {
          owner.attempt = null;
          owner.review = null;
          owner.error =
            "This playlist or its connection changed after review. Review the current connection again before resuming updates.";
        } else {
          owner.error =
            "Hito could not confirm whether updates resumed. Retry the same request to recover its result.";
        }
        log.warn("playlist_follow.resume_failed", {
          ...fields,
          reason: error instanceof HttpError ? "http" : "request",
          status: error instanceof HttpError ? error.status : undefined,
          duration_ms: Math.round(performance.now() - started),
        });
      }
    } finally {
      if (owner.run === run) {
        owner.run = null;
        if (request.active()) publish();
      }
      if (!request.active() || signal.aborted)
        log.debug("playlist_follow.resume_cancelled", fields);
    }
    if (accepted && request.active()) await recheck.current();
    return accepted;
  }

  async function resume(): Promise<boolean> {
    const review = owner.review;
    if (
      !playlist ||
      !review ||
      owner.run ||
      owner.attempt ||
      !scope.capture().active()
    )
      return false;
    try {
      const batch = prepareChange({
        kind: "playlist_follow_resume",
        playlist_id: playlist,
        expected_generation: review.generation,
        installation_revision: review.installation.revision,
        service_revision: review.service.revision,
      });
      return perform(batch, false);
    } catch {
      owner.error =
        "Hito could not prepare this approval. Review the connection again.";
      publish();
      log.warn("playlist_follow.resume_rejected", {
        account_id: account,
        playlist_id: playlist,
        reason: "prepare",
      });
      return false;
    }
  }

  return {
    review: current?.review ?? null,
    pending: current?.pending ?? false,
    hasAttempt: current?.hasAttempt ?? false,
    error: current?.error ?? "",
    reviewConnection,
    resume,
    retry: () =>
      owner.attempt ? perform(owner.attempt, true) : Promise.resolve(false),
  };
}
