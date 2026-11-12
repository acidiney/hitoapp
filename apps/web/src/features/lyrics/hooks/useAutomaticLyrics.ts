import { useState } from "react";
import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import { lookupAutomatic, rememberedAutomatic } from "../api/automaticLyrics";
import type { AutomaticOutcome } from "../model/lyricContent";
import {
  changedFailure,
  deadlineFailure,
  lyricFailure,
  type LyricFailure,
} from "../model/lyricFailure";
import { useLatestRun } from "./useLatestRun";

type State = {
  owner: object;
  pending: boolean;
  outcome: AutomaticOutcome | null;
  failure: LyricFailure | null;
};

export function useAutomaticLyrics(
  account: string,
  recording: string,
  onFailure: (failure: LyricFailure) => void,
) {
  const run = useLatestRun(JSON.stringify([account, recording]));
  const fresh = (owner: object): State => ({
    owner,
    pending: false,
    outcome: rememberedAutomatic(account, recording),
    failure: null,
  });
  const [snapshot, setSnapshot] = useState(() => fresh(run.owner));
  const current = snapshot.owner === run.owner ? snapshot : fresh(run.owner);

  function cancel() {
    run.cancel();
    setSnapshot(fresh(run.owner));
  }

  function retire() {
    if (current.pending) cancel();
  }

  async function lookup() {
    const started = performance.now();
    const requestId = createUuid();
    const active = run.start(30_000, () =>
      setSnapshot({
        ...fresh(run.owner),
        outcome: null,
        failure: deadlineFailure("search"),
      }),
    );
    if (!active) return;
    let reason = "cancelled";
    setSnapshot({
      owner: run.owner,
      pending: true,
      outcome: null,
      failure: null,
    });
    log.debug("lyrics.automatic.started", {
      action: "automatic",
      request_id: requestId,
    });
    try {
      const result = await lookupAutomatic(account, recording, active.signal);
      if (!active.active()) return;
      if (!result) {
        reason = "changed";
        const failure = changedFailure();
        setSnapshot({
          owner: run.owner,
          pending: false,
          outcome: null,
          failure,
        });
        onFailure(failure);
        return;
      }
      reason = result.kind;
      setSnapshot({
        owner: run.owner,
        pending: false,
        outcome: result.kind === "cancelled" ? null : result,
        failure: null,
      });
    } catch (error) {
      if (!active.active()) return;
      reason = "failed";
      const failure = lyricFailure(error, "search");
      setSnapshot({
        owner: run.owner,
        pending: false,
        outcome: null,
        failure,
      });
      onFailure(failure);
    } finally {
      active.finish();
      log.debug("lyrics.automatic.finished", {
        action: "automatic",
        request_id: requestId,
        reason,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  return {
    pending: current.pending,
    outcome: current.outcome,
    failure: current.failure,
    lookup,
    cancel,
    retire,
  };
}
