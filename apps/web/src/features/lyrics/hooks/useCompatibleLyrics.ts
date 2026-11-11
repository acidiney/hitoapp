import { useState } from "react";
import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import {
  listCompatibleChoices,
  readCompatibleChoice,
  type CompatibleChoice,
} from "../api/compatibleLyrics";
import type { SelectedLyricsOutcome } from "../model/lyricContent";
import {
  changedFailure,
  deadlineFailure,
  lyricFailure,
  type LyricFailure,
} from "../model/lyricFailure";
import { useLatestRun } from "./useLatestRun";

type State = {
  owner: object;
  attempted: boolean;
  pending: boolean;
  choices: CompatibleChoice[];
  chosenKey: string | null;
  outcome: SelectedLyricsOutcome | null;
  failure: LyricFailure | null;
};

function empty(owner: object): State {
  return {
    owner,
    attempted: false,
    pending: false,
    choices: [],
    chosenKey: null,
    outcome: null,
    failure: null,
  };
}

export function useCompatibleLyrics(
  account: string,
  recording: string,
  onFailure: (failure: LyricFailure) => void,
) {
  const run = useLatestRun(JSON.stringify([account, recording]));
  const [snapshot, setSnapshot] = useState(() => empty(run.owner));
  const current = snapshot.owner === run.owner ? snapshot : empty(run.owner);

  function cancel() {
    run.cancel();
    setSnapshot(empty(run.owner));
  }

  async function lookup(chosenKey?: string) {
    const started = performance.now();
    const requestId = createUuid();
    const known = current.choices;
    const active = run.start(15_000, () =>
      setSnapshot({
        ...empty(run.owner),
        attempted: true,
        choices: known,
        failure: deadlineFailure("read"),
      }),
    );
    if (!active) return;
    let reason = "cancelled";
    setSnapshot({
      ...empty(run.owner),
      attempted: true,
      pending: true,
      choices: known,
    });
    log.debug("lyrics.compatible.started", {
      action: "lookup",
      request_id: requestId,
    });
    try {
      const listed =
        chosenKey && known.length > 0
          ? known
          : await listCompatibleChoices(account, recording, active.signal);
      if (!active.active()) return;
      if (listed === "changed") {
        reason = "changed";
        const failure = changedFailure();
        setSnapshot({ ...empty(run.owner), attempted: true, failure });
        onFailure(failure);
        return;
      }
      const choice =
        listed.find((item) => item.key === chosenKey) ?? listed[0] ?? null;
      if (!choice) {
        reason = "not_found";
        setSnapshot({
          ...empty(run.owner),
          attempted: true,
          outcome: { kind: "not_found" },
        });
        return;
      }
      const read = await readCompatibleChoice(
        account,
        recording,
        choice,
        active.signal,
      );
      if (!active.active()) return;
      if (read.kind === "changed") {
        reason = "changed";
        const failure = changedFailure();
        setSnapshot({
          ...empty(run.owner),
          attempted: true,
          choices: listed,
          failure,
        });
        onFailure(failure);
        return;
      }
      reason = read.outcome.kind;
      setSnapshot({
        ...empty(run.owner),
        attempted: true,
        choices: listed,
        chosenKey: choice.key,
        outcome: read.outcome.kind === "cancelled" ? null : read.outcome,
      });
    } catch (error) {
      if (!active.active()) return;
      reason = "failed";
      const failure = lyricFailure(error, "read");
      setSnapshot({
        ...empty(run.owner),
        attempted: true,
        choices: known,
        failure,
      });
      onFailure(failure);
    } finally {
      active.finish();
      log.debug("lyrics.compatible.finished", {
        action: "lookup",
        request_id: requestId,
        reason,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  return {
    attempted: current.attempted,
    pending: current.pending,
    choices: current.choices,
    chosenKey: current.chosenKey,
    outcome: current.outcome,
    failure: current.failure,
    lookup,
    cancel,
  };
}
