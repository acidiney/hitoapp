import { useState } from "react";
import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import { lyricsApi } from "../api/lyricsApi";
import {
  deadlineFailure,
  lyricFailure,
  type LyricFailure,
} from "../model/lyricFailure";
import {
  identityKey,
  isRemovable,
  type LyricIdentity,
} from "../model/lyricIdentity";
import { useLatestRun } from "./useLatestRun";

type State = {
  owner: object;
  removing: string | null;
  failure: (LyricFailure & { key: string }) | null;
};

function empty(owner: object): State {
  return { owner, removing: null, failure: null };
}

export function useLyricRemoval(
  account: string,
  recording: string,
  onFailure: (failure: LyricFailure) => void,
) {
  const run = useLatestRun(JSON.stringify([account, recording]));
  const [snapshot, setSnapshot] = useState(() => empty(run.owner));
  const current = snapshot.owner === run.owner ? snapshot : empty(run.owner);

  function clear() {
    run.cancel();
    setSnapshot(empty(run.owner));
  }

  async function remove(identity: LyricIdentity, onSettled: () => void) {
    if (!isRemovable(identity)) return;
    const key = identityKey(identity);
    const started = performance.now();
    const requestId = createUuid();
    const active = run.start(15_000, () =>
      setSnapshot({
        ...empty(run.owner),
        failure: { ...deadlineFailure("remove"), key },
      }),
    );
    if (!active) return;
    let outcome = "cancelled";
    setSnapshot({ ...empty(run.owner), removing: key });
    log.debug("lyrics.remove.started", {
      action: "remove",
      request_id: requestId,
    });
    try {
      await lyricsApi.removeIdentity(
        account,
        recording,
        {
          id: identity.selector.identity_id,
          revision: identity.selector.identity_revision,
        },
        active.signal,
      );
      if (!active.active()) return;
      outcome = "removed";
      setSnapshot(empty(run.owner));
      onSettled();
    } catch (error) {
      if (!active.active()) return;
      outcome = "failed";
      const failure = lyricFailure(error, "remove");
      setSnapshot({ ...empty(run.owner), failure: { ...failure, key } });
      onFailure(failure);
      onSettled();
    } finally {
      active.finish();
      log.debug("lyrics.remove.finished", {
        action: "remove",
        request_id: requestId,
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  return {
    removing: current.removing,
    failure: current.failure,
    remove,
    clear,
  };
}
