import type { Schema } from "../../../shared/schema";
import { useEffect, useMemo, useRef, useState } from "react";
import { prepareChange, submitChange } from "../../../shared/sync";
import { HttpError } from "../../../shared/http";
import { log } from "../../../shared/log";

import { useRequestScope } from "../../../shared/hooks/useRequestScope";

type ResumeOwner = {
  run: AbortController | null;
  attempt: Schema<"SyncBatch"> | null;
  error: string;
};
type ResumeState = {
  owner: ResumeOwner;
  pending: boolean;
  hasAttempt: boolean;
  error: string;
};

export function useFollowResume(
  account: string,
  playlist: string | null,
  follow: Schema<"PlaylistFollow"> | null,
  recheckStatus: () => Promise<void>,
) {
  const key = JSON.stringify([account, playlist]);
  const scope = useRequestScope(key);
  const owner = useMemo<ResumeOwner>(
    () => ({ run: null, attempt: null, error: "" }),
    [key],
  );
  const recheck = useRef(recheckStatus);
  recheck.current = recheckStatus;
  const [state, setState] = useState<ResumeState | null>(null);
  const current = state?.owner === owner ? state : null;

  function publish() {
    setState({
      owner,
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

  async function perform(
    batch: Schema<"SyncBatch">,
    retry: boolean,
  ): Promise<boolean> {
    const request = scope.capture();
    if (!playlist || !request.active() || owner.run) return false;
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
    let settled = false;
    log.info("playlist_follow.resume_started", fields);
    try {
      await submitChange(account, batch, signal);
      if (!active()) return false;
      owner.attempt = null;
      settled = true;
      log.info("playlist_follow.resume_accepted", {
        ...fields,
        duration_ms: Math.round(performance.now() - started),
      });
    } catch (error) {
      if (active()) {
        if (error instanceof HttpError && error.status === 409) {
          owner.attempt = null;
          settled = true;
          owner.error =
            "This playlist or its connection changed. Check what changed, then resume updates again.";
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
    const accepted = settled && owner.error === "";
    if (settled && request.active()) await recheck.current();
    return accepted;
  }

  async function resume(): Promise<boolean> {
    const review = follow?.review;
    if (
      !playlist ||
      !follow ||
      follow.status !== "paused" ||
      !review?.resumable ||
      owner.run ||
      owner.attempt ||
      !scope.capture().active()
    )
      return false;
    try {
      return perform(
        prepareChange({
          kind: "playlist_follow_resume",
          playlist_id: playlist,
          expected_generation: follow.generation,
          installation_revision: review.installation_revision,
          service_revision: review.service_revision,
        }),
        false,
      );
    } catch {
      owner.error =
        "Hito could not prepare this approval. Check the connection and try again.";
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
    pending: current?.pending ?? false,
    hasAttempt: current?.hasAttempt ?? false,
    error: current?.error ?? "",
    resume,
    retry: () =>
      owner.attempt ? perform(owner.attempt, true) : Promise.resolve(false),
  };
}
