import { useState } from "react";
import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import type { Schema } from "../../../shared/schema";
import { lyricsApi } from "../api/lyricsApi";
import {
  deadlineFailure,
  lyricFailure,
  type LyricFailure,
} from "../model/lyricFailure";
import type { LyricService } from "../model/lyricIdentity";
import { useLatestRun } from "./useLatestRun";

export type LyricCandidate = Schema<"ProviderRecording">;

type State = {
  owner: object;
  pending: boolean;
  candidates: LyricCandidate[] | null;
  failure: LyricFailure | null;
};

function empty(owner: object): State {
  return { owner, pending: false, candidates: null, failure: null };
}

export function useLyricSearch(
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

  function cancel() {
    run.cancel();
    setSnapshot(empty(run.owner));
  }

  async function search(query: string) {
    const text = query.trim();
    if (!service || text === "") return;
    const started = performance.now();
    const requestId = createUuid();
    const active = run.start(15_000, () =>
      setSnapshot({ ...empty(run.owner), failure: deadlineFailure("search") }),
    );
    if (!active) return;
    let outcome = "cancelled";
    setSnapshot({ ...empty(run.owner), pending: true });
    log.debug("lyrics.search.started", {
      action: "search",
      request_id: requestId,
    });
    try {
      const page = await lyricsApi.candidates(
        account,
        recording,
        {
          installation_id: service.installation_id,
          installation_revision: service.installation_revision,
          query: text,
        },
        active.signal,
      );
      if (!active.active()) return;
      outcome = page.items.length === 0 ? "empty" : "found";
      setSnapshot({ ...empty(run.owner), candidates: page.items });
    } catch (error) {
      if (!active.active()) return;
      outcome = "failed";
      const failure = lyricFailure(error, "search");
      setSnapshot({ ...empty(run.owner), failure });
      onFailure(failure);
    } finally {
      active.finish();
      if (active.active())
        setSnapshot((value) =>
          value.owner === run.owner ? { ...value, pending: false } : value,
        );
      log.debug("lyrics.search.finished", {
        action: "search",
        request_id: requestId,
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  return {
    pending: current.pending,
    candidates: current.candidates,
    failure: current.failure,
    search,
    cancel,
  };
}
