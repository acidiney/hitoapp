import type { Schema } from "../../../shared/schema";
import { useEffect, useMemo, useRef, useState } from "react";
import { errorMessage } from "../../../shared/format";
import { log } from "../../../shared/log";

import { createUuid } from "../../../shared/uuid";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { similarArtistsApi } from "../api/similarArtistsApi";

const REQUEST_DEADLINE_MS = 20_000;
const LIMIT = 10;

type Owner = { pending: boolean };
type State = {
  owner: Owner;
  result: Schema<"SimilarArtistsAvailability"> | null;
  pending: boolean;
  error: string;
};

function empty(owner: Owner): State {
  return { owner, result: null, pending: false, error: "" };
}

export function useSimilarArtists(
  account: string,
  group: Schema<"ArtistIdentityGroupDetail">["group"],
  auto = false,
) {
  const scope = useRequestScope(
    JSON.stringify([
      account,
      group.id,
      group.provider,
      group.provider_artist_id,
      group.name.recording_id,
      group.name.association_id,
    ]),
  );
  const owner = useMemo<Owner>(() => ({ pending: false }), [scope]);
  const [snapshot, setSnapshot] = useState(() => empty(owner));
  const current = snapshot.owner === owner ? snapshot : empty(owner);

  async function load() {
    const request = scope.capture();
    if (!request.active() || owner.pending) return;
    owner.pending = true;
    const started = performance.now();
    const requestId = createUuid();
    const controller = new AbortController();
    let timedOut = false;
    let outcome = "cancelled";
    const cancel = () => controller.abort(request.signal.reason);
    request.signal.addEventListener("abort", cancel, { once: true });
    const timer = setTimeout(() => {
      timedOut = true;
      controller.abort(new Error("Similar artists timed out. Try again."));
    }, REQUEST_DEADLINE_MS);
    setSnapshot({ ...empty(owner), pending: true });
    log.debug("similar_artists.started", { request_id: requestId });
    try {
      const result = await similarArtistsApi.read(
        account,
        group.name.recording_id,
        group.name.association_id,
        { limit: LIMIT },
        controller.signal,
      );
      if (!request.active()) return;
      outcome = result.status;
      setSnapshot({
        owner,
        result: result.status === "unavailable" ? null : result,
        pending: false,
        error:
          result.status === "unavailable"
            ? "Similar artists are unavailable. Try again."
            : "",
      });
    } catch (error) {
      if (request.active()) {
        outcome = timedOut ? "timed_out" : "failed";
        setSnapshot({
          ...empty(owner),
          error: timedOut
            ? "Similar artists timed out. Try again."
            : errorMessage(error),
        });
      }
    } finally {
      clearTimeout(timer);
      request.signal.removeEventListener("abort", cancel);
      owner.pending = false;
      const record =
        outcome === "failed" || outcome === "timed_out" ? log.warn : log.debug;
      record("similar_artists.finished", {
        request_id: requestId,
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  const latest = useRef(load);
  latest.current = load;

  useEffect(() => {
    if (!auto) return;
    let active = true;
    queueMicrotask(() => {
      if (active) void latest.current();
    });
    return () => {
      active = false;
    };
  }, [auto, owner, scope]);

  return { ...current, load };
}
