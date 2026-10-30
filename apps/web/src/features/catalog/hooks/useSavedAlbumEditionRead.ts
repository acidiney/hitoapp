import type { Schema } from "../../../shared/schema";
import { useEffect, useMemo, useState } from "react";
import { errorMessage } from "../../../shared/format";
import { log } from "../../../shared/log";

import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { albumEditionChoiceApi } from "../api/albumEditionChoiceApi";

const REQUEST_DEADLINE_MS = 20_000;

export type SavedRead = Extract<
  Schema<"AlbumEditionTrackListAvailability">,
  { status: "available" }
>;
type State = {
  owner: object;
  comparison: SavedRead | null;
  pending: boolean;
  error: string;
};

const messages = {
  disabled: "Metadata providers are disabled in your settings.",
  unsupported: "This provider does not support complete edition track lists.",
  unavailable: "This edition's track list is unavailable. Try again.",
  cancelled: "The edition track list request was cancelled.",
};

export function useSavedAlbumEditionRead(
  account: string,
  release: string,
  stale: () => void,
) {
  const scope = useRequestScope(`${account}/${release}`);
  const owner = useMemo(() => ({}), [scope]);
  const [snapshot, setSnapshot] = useState<State>(() => ({
    owner,
    comparison: null,
    pending: false,
    error: "",
  }));
  const current: State =
    snapshot.owner === owner
      ? snapshot
      : { owner, comparison: null, pending: false, error: "" };

  async function load() {
    const request = scope.capture();
    if (!request.active()) return;
    const controller = new AbortController();
    const cancel = () => controller.abort();
    request.signal.addEventListener("abort", cancel, { once: true });
    const timer = setTimeout(() => controller.abort(), REQUEST_DEADLINE_MS);
    const started = performance.now();
    let outcome = "cancelled";
    let count = 0;
    setSnapshot({ owner, comparison: null, pending: true, error: "" });
    try {
      const result = await albumEditionChoiceApi.tracks(
        account,
        release,
        controller.signal,
      );
      if (!request.active()) return;
      if (result.status === "needs_choice") {
        outcome = "needs_choice";
        setSnapshot({ owner, comparison: null, pending: false, error: "" });
        stale();
        return;
      }
      if (result.status !== "available") {
        outcome = result.status;
        throw new Error(messages[result.status]);
      }
      count = result.edition.media.reduce(
        (total, medium) => total + medium.tracks.length,
        0,
      );
      outcome = "accepted";
      setSnapshot({ owner, comparison: result, pending: false, error: "" });
    } catch (failure) {
      if (!request.active()) return;
      if (outcome === "cancelled") outcome = "failed";
      setSnapshot({
        owner,
        comparison: null,
        pending: false,
        error: controller.signal.aborted
          ? "This edition track list request timed out. Try again."
          : errorMessage(failure),
      });
    } finally {
      clearTimeout(timer);
      request.signal.removeEventListener("abort", cancel);
      const record = outcome === "failed" ? log.warn : log.debug;
      record("album_edition.saved_read.finished", {
        reason: outcome,
        count,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  useEffect(() => {
    let active = true;
    queueMicrotask(() => {
      if (active) void load();
    });
    return () => {
      active = false;
    };
  }, [owner]);

  return { ...current, retry: load };
}
