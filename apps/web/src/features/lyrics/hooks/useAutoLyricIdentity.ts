import { useState } from "react";
import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import {
  autoLookupFinished,
  findAutoIdentity,
  rememberAutoLookup,
} from "../api/autoIdentity";
import {
  deadlineFailure,
  lyricFailure,
  type LyricFailure,
} from "../model/lyricFailure";
import type { LyricIdentity } from "../model/lyricIdentity";
import type { RecordingFacts } from "../model/lyricMatch";
import { useLatestRun } from "./useLatestRun";

type State = {
  owner: object;
  pending: boolean;
  noMatch: boolean;
  failure: LyricFailure | null;
};

function empty(owner: object, noMatch: boolean): State {
  return { owner, pending: false, noMatch, failure: null };
}

export function useAutoLyricIdentity(
  account: string,
  recording: string,
  facts: RecordingFacts,
  onFailure: (failure: LyricFailure) => void,
) {
  const run = useLatestRun(JSON.stringify([account, recording]));
  const remembered = autoLookupFinished(account, recording) !== null;
  const [snapshot, setSnapshot] = useState(() => empty(run.owner, remembered));
  const current =
    snapshot.owner === run.owner ? snapshot : empty(run.owner, remembered);

  function cancel() {
    run.cancel();
    setSnapshot(
      empty(run.owner, autoLookupFinished(account, recording) !== null),
    );
  }

  async function lookup(onSaved: (identity: LyricIdentity) => void) {
    const started = performance.now();
    const requestId = createUuid();
    const active = run.start(30_000, () =>
      setSnapshot({
        ...empty(run.owner, false),
        failure: deadlineFailure("search"),
      }),
    );
    if (!active) return;
    let reason = "cancelled";
    setSnapshot({ ...empty(run.owner, false), pending: true });
    log.debug("lyrics.auto.started", { action: "auto", request_id: requestId });
    try {
      const result = await findAutoIdentity(
        account,
        recording,
        facts,
        active.signal,
      );
      if (!active.active()) return;
      reason = result.kind;
      if (result.kind === "saved") {
        setSnapshot(empty(run.owner, false));
        onSaved(result.identity);
        return;
      }
      if (result.kind === "none") rememberAutoLookup(account, recording);
      setSnapshot(empty(run.owner, true));
    } catch (error) {
      if (!active.active()) return;
      reason = "failed";
      const failure = lyricFailure(error, "search");
      setSnapshot({ ...empty(run.owner, false), failure });
      onFailure(failure);
    } finally {
      active.finish();
      log.debug("lyrics.auto.finished", {
        action: "auto",
        request_id: requestId,
        reason,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  return {
    pending: current.pending,
    noMatch: current.noMatch,
    failure: current.failure,
    lookup,
    cancel,
  };
}
