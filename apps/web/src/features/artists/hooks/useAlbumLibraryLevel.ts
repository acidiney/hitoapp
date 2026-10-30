import type { Schema } from "../../../shared/schema";
import { useMemo, useState } from "react";
import { albumEditionApi } from "../api/albumEditionApi";
import { errorMessage } from "../../../shared/format";
import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { DeadlineError, withDeadline } from "./withDeadline";

const REQUEST_DEADLINE_MS = 20_000;

type Owner = { pending: boolean };
type State = {
  owner: Owner;
  pending: boolean;
  level: Schema<"AlbumLibraryLevel"> | null;
  error: string;
};

const empty = (owner: Owner): State => ({
  owner,
  pending: false,
  level: null,
  error: "",
});

const messages: Record<
  Exclude<Schema<"AlbumLibraryLevel">["status"], "compared">,
  string
> = {
  needs_choice: "Choose an edition on the album page to compare it.",
  disabled: "Metadata providers are disabled in your settings.",
  unsupported: "This provider cannot list the tracks of an edition.",
  unavailable: "The edition could not be read. Try again.",
  cancelled: "The comparison was cancelled. Try again.",
};

/** One explicit comparison of a library album with the edition remembered for it. */
export function useAlbumLibraryLevel(
  account: string,
  release: string,
  releaseGroup: string,
) {
  const scope = useRequestScope(
    JSON.stringify([account, release, releaseGroup]),
  );
  const owner = useMemo<Owner>(() => ({ pending: false }), [scope]);
  const [snapshot, setSnapshot] = useState(() => empty(owner));
  const current = snapshot.owner === owner ? snapshot : empty(owner);

  async function compare() {
    const request = scope.capture();
    if (!request.active() || owner.pending) return;
    owner.pending = true;
    const requestId = createUuid();
    const started = performance.now();
    let outcome = "cancelled";
    setSnapshot({ owner, pending: true, level: null, error: "" });
    try {
      const result = await withDeadline(
        request.signal,
        REQUEST_DEADLINE_MS,
        (signal) =>
          albumEditionApi.libraryLevel(
            account,
            release,
            { release_group_id: releaseGroup },
            signal,
          ),
      );
      if (!request.active()) return;
      outcome = result.status;
      setSnapshot({
        owner,
        pending: false,
        level: result.status === "compared" ? result : null,
        error: result.status === "compared" ? "" : messages[result.status],
      });
    } catch (error) {
      if (!request.active()) return;
      outcome = error instanceof DeadlineError ? "timed_out" : "failed";
      setSnapshot({
        owner,
        pending: false,
        level: null,
        error: errorMessage(error),
      });
    } finally {
      owner.pending = false;
      (outcome === "failed" || outcome === "timed_out" ? log.warn : log.debug)(
        "album_library_level.finished",
        {
          request_id: requestId,
          reason: outcome,
          duration_ms: Math.round(performance.now() - started),
        },
      );
    }
  }

  return {
    pending: current.pending,
    level: current.level,
    error: current.error,
    retryable: current.error !== "" && current.error !== messages.needs_choice,
    compare,
  };
}
