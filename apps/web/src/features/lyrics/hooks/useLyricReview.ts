import { useState } from "react";
import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import type { Schema } from "../../../shared/schema";
import { lyricsApi } from "../api/lyricsApi";
import {
  changedFailure,
  deadlineFailure,
  lyricFailure,
  type LyricFailure,
  type LyricStep,
} from "../model/lyricFailure";
import type { LyricIdentity, LyricService } from "../model/lyricIdentity";
import { useLatestRun } from "./useLatestRun";
import type { LyricCandidate } from "./useLyricSearch";

type Review = Schema<"RecordingLyricReview">;
type Phase = "idle" | "reviewing" | "reviewed" | "accepting";
type State = {
  owner: object;
  phase: Phase;
  candidate: LyricCandidate | null;
  review: Review | null;
  failure: LyricFailure | null;
};

function empty(owner: object): State {
  return {
    owner,
    phase: "idle",
    candidate: null,
    review: null,
    failure: null,
  };
}

export function useLyricReview(
  account: string,
  recording: string,
  service: LyricService | null,
  onFailure: (failure: LyricFailure) => void,
) {
  const run = useLatestRun(
    JSON.stringify([
      account,
      recording,
      service?.installation_id,
      service?.installation_revision,
    ]),
  );
  const [snapshot, setSnapshot] = useState(() => empty(run.owner));
  const current = snapshot.owner === run.owner ? snapshot : empty(run.owner);

  function clear() {
    run.cancel();
    setSnapshot(empty(run.owner));
  }

  async function inFlight<T>(
    step: LyricStep,
    candidate: LyricCandidate,
    idle: Phase,
    work: (signal: AbortSignal) => Promise<T>,
    done: (result: T) => State | null,
  ) {
    const started = performance.now();
    const requestId = createUuid();
    const active = run.start(15_000, () =>
      setSnapshot({
        ...empty(run.owner),
        candidate,
        failure: deadlineFailure(step),
      }),
    );
    if (!active) return;
    let outcome = "cancelled";
    setSnapshot((value) => ({
      ...(value.owner === run.owner ? value : empty(run.owner)),
      candidate,
      phase: idle,
      failure: null,
    }));
    log.debug("lyrics.review.started", { action: step, request_id: requestId });
    try {
      const result = await work(active.signal);
      if (!active.active()) return;
      const next = done(result);
      outcome = next?.failure ? next.failure.kind : "ok";
      if (next) setSnapshot(next);
      if (next?.failure) onFailure(next.failure);
    } catch (error) {
      if (!active.active()) return;
      outcome = "failed";
      const failure = lyricFailure(error, step);
      setSnapshot({ ...empty(run.owner), candidate, failure });
      onFailure(failure);
    } finally {
      active.finish();
      log.debug("lyrics.review.finished", {
        action: step,
        request_id: requestId,
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  async function choose(candidate: LyricCandidate) {
    if (!service) return;
    run.cancel();
    await inFlight(
      "review",
      candidate,
      "reviewing",
      (signal) =>
        lyricsApi.review(
          account,
          recording,
          {
            installation_id: service.installation_id,
            installation_revision: service.installation_revision,
            provider_recording_id: candidate.id,
          },
          signal,
        ),
      (review) =>
        review.candidate.recording.id !== candidate.id
          ? { ...empty(run.owner), candidate, failure: changedFailure() }
          : { ...empty(run.owner), candidate, phase: "reviewed", review },
    );
  }

  async function accept(onAccepted: (identity: LyricIdentity) => void) {
    const { candidate, review } = current;
    if (!service || !candidate || !review || current.phase !== "reviewed")
      return;
    await inFlight(
      "accept",
      candidate,
      "accepting",
      (signal) =>
        lyricsApi.retain(
          account,
          recording,
          {
            candidate: {
              installation_id: service.installation_id,
              installation_revision: service.installation_revision,
              provider_recording_id: candidate.id,
            },
            review_stamp: review.review_stamp,
          },
          signal,
        ),
      (identity) => {
        onAccepted(identity);
        return empty(run.owner);
      },
    );
  }

  return {
    phase: current.phase,
    candidate: current.candidate,
    review: current.review,
    failure: current.failure,
    choose,
    accept,
    clear,
  };
}
