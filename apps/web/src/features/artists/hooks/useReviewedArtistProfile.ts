import { useEffect, useMemo, useState } from "react";
import { artistReviewApi } from "../api/artistReviewApi";
import { errorMessage } from "../../../shared/format";
import { log } from "../../../shared/log";
import type {
  ArtistAssociation,
  CatalogArtistProfile,
} from "../../../shared/types";
import { createUuid } from "../../../shared/uuid";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";

const REQUEST_DEADLINE_MS = 20_000;
const CHANGED_ARTIST =
  "This album's reviewed artist has changed. Reload the page to compare it with a catalog.";

type Owner = { pending: boolean };
export type ReviewedArtist = {
  association: ArtistAssociation;
  profile: CatalogArtistProfile;
};
type State = {
  owner: Owner;
  ready: ReviewedArtist | null;
  pending: boolean;
  error: string;
};

function empty(owner: Owner): State {
  return { owner, ready: null, pending: false, error: "" };
}

function unavailable(status: string) {
  switch (status) {
    case "disabled":
      return "Metadata providers are disabled in your settings.";
    case "unsupported":
      return "This provider does not support artist profiles.";
    case "cancelled":
      return "The artist information request was cancelled.";
    default:
      return "Artist information is unavailable. Try again.";
  }
}

export function useReviewedArtistProfile(
  account: string,
  anchor: { recording_id: string; association_id: string },
) {
  const scope = useRequestScope(
    JSON.stringify([account, anchor.recording_id, anchor.association_id]),
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
    let stop!: (error: Error) => void;
    const stopped = new Promise<never>((_, reject) => {
      stop = reject;
    });
    let timedOut = false;
    let outcome = "cancelled";
    const cancel = () => {
      const error = new DOMException(
        "The artist information request was cancelled",
        "AbortError",
      );
      controller.abort(error);
      stop(error);
    };
    const check = () => {
      if (!request.active()) cancel();
      controller.signal.throwIfAborted();
    };
    request.signal.addEventListener("abort", cancel, { once: true });
    const timer = setTimeout(() => {
      timedOut = true;
      const error = new Error("Artist information timed out. Try again.");
      controller.abort(error);
      stop(error);
    }, REQUEST_DEADLINE_MS);
    if (request.signal.aborted) cancel();
    setSnapshot({ ...empty(owner), pending: true });
    log.debug("reviewed_artist_profile.started", { request_id: requestId });
    try {
      const ready = await Promise.race([
        (async () => {
          const associations = await artistReviewApi.associations(
            account,
            anchor.recording_id,
            controller.signal,
          );
          check();
          const association = associations.find(
            (item) => item.id === anchor.association_id,
          );
          if (
            !association ||
            association.account_id !== account ||
            association.recording_id !== anchor.recording_id
          )
            throw new Error(CHANGED_ARTIST);
          const result = await artistReviewApi.profile(
            account,
            anchor.recording_id,
            association.id,
            controller.signal,
          );
          check();
          if (result.status !== "available") {
            outcome = result.status;
            throw new Error(unavailable(result.status));
          }
          if (
            result.association_id !== association.id ||
            result.profile.artist.id !== association.provider_artist_id
          )
            throw new Error(CHANGED_ARTIST);
          return { association, profile: result.profile };
        })(),
        stopped,
      ]);
      check();
      setSnapshot({ owner, ready, pending: false, error: "" });
      outcome = "accepted";
    } catch (error) {
      if (request.active()) {
        if (outcome === "cancelled")
          outcome = timedOut ? "timed_out" : "failed";
        setSnapshot({
          ...empty(owner),
          error: timedOut
            ? "Artist information timed out. Try again."
            : errorMessage(error),
        });
      }
    } finally {
      clearTimeout(timer);
      request.signal.removeEventListener("abort", cancel);
      owner.pending = false;
      const record =
        outcome === "failed" || outcome === "timed_out" ? log.warn : log.debug;
      record("reviewed_artist_profile.finished", {
        request_id: requestId,
        reason: outcome,
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
  }, [owner, scope]);

  return { ...current, retry: load };
}
