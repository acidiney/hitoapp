import { useState } from "react";
import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import { lyricsApi } from "../api/lyricsApi";
import {
  readMatchesIdentity,
  selectedLyricsOutcome,
  type SelectedLyricsOutcome,
} from "../model/lyricContent";
import {
  changedFailure,
  deadlineFailure,
  lyricFailure,
  type LyricFailure,
} from "../model/lyricFailure";
import { identityKey, type LyricIdentity } from "../model/lyricIdentity";
import { useLatestRun } from "./useLatestRun";
import {
  clearCredentialRequirementAfterProtectedSuccess,
  credentialMarkVersion,
} from "../../plugins/model/credentialRequirements";

type State = {
  owner: object;
  pending: boolean;
  outcome: SelectedLyricsOutcome | null;
  failure: LyricFailure | null;
};

function empty(owner: object): State {
  return { owner, pending: false, outcome: null, failure: null };
}

export function useSelectedLyrics(
  account: string,
  recording: string,
  identity: LyricIdentity | null,
  onFailure: (failure: LyricFailure) => void,
) {
  const run = useLatestRun(
    JSON.stringify([
      account,
      recording,
      identity && identityKey(identity),
      identity?.installation_id,
      identity?.installation_revision,
      identity?.provider_recording.id,
      identity?.provider_recording.revision,
    ]),
  );
  const [snapshot, setSnapshot] = useState(() => empty(run.owner));
  const current = snapshot.owner === run.owner ? snapshot : empty(run.owner);

  function cancel() {
    run.cancel();
    setSnapshot(empty(run.owner));
  }

  async function load() {
    if (!identity?.selectable) return;
    const started = performance.now();
    const requestId = createUuid();
    const active = run.start(15_000, () =>
      setSnapshot({ ...empty(run.owner), failure: deadlineFailure("read") }),
    );
    if (!active) return;
    let outcome = "cancelled";
    setSnapshot({ ...empty(run.owner), pending: true });
    log.debug("lyrics.read.started", {
      action: "read_selected",
      request_id: requestId,
    });
    const mark = credentialMarkVersion(account, identity.installation_id);
    try {
      const result = await lyricsApi.readSelected(
        account,
        recording,
        {
          selector: identity.selector,
          installation_revision: identity.installation_revision,
        },
        active.signal,
      );
      if (!active.active()) return;
      if (
        (result.status === "ready_v1" || result.status === "ready_v2") &&
        !readMatchesIdentity(result, recording, identity)
      ) {
        outcome = "changed";
        const failure = changedFailure();
        setSnapshot({ ...empty(run.owner), failure });
        onFailure(failure);
        return;
      }
      if (result.status === "ready_v1" || result.status === "ready_v2")
        clearCredentialRequirementAfterProtectedSuccess(
          account,
          identity.installation_id,
          identity.installation_revision,
          mark,
        );
      const shown = selectedLyricsOutcome(result);
      outcome = shown.kind;
      setSnapshot({
        ...empty(run.owner),
        outcome: shown.kind === "cancelled" ? null : shown,
      });
    } catch (error) {
      if (!active.active()) return;
      outcome = "failed";
      const failure = lyricFailure(error, "read");
      setSnapshot({ ...empty(run.owner), failure });
      onFailure(failure);
    } finally {
      active.finish();
      if (active.active())
        setSnapshot((value) =>
          value.owner === run.owner ? { ...value, pending: false } : value,
        );
      log.debug("lyrics.read.finished", {
        action: "read_selected",
        request_id: requestId,
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  return {
    pending: current.pending,
    outcome: current.outcome,
    failure: current.failure,
    load,
    cancel,
  };
}
