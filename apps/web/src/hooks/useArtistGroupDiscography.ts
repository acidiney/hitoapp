import { useMemo, useState } from "react";
import { artistReviewApi } from "../artistReviewApi";
import { errorMessage } from "../format";
import { log } from "../log";
import type {
  ArtistAssociation,
  ArtistIdentityGroupDetail,
  ArtistProfileAvailability,
  CatalogArtistProfile,
  MetadataProvider,
} from "../types";
import { createUuid } from "../uuid";
import { useRequestScope } from "./useRequestScope";

const REQUEST_DEADLINE_MS = 20_000;
const CHANGED_ARTIST =
  "This chosen artist has changed. Reload the page to browse its discography.";

type Owner = { pending: boolean };
type Ready = {
  association: ArtistAssociation;
  profile: CatalogArtistProfile;
};
type State = {
  owner: Owner;
  ready: Ready | null;
  pending: boolean;
  error: string;
};

function empty(owner: Owner): State {
  return { owner, ready: null, pending: false, error: "" };
}

function sameProvider(left: MetadataProvider, right: MetadataProvider) {
  return (
    left.kind === right.kind &&
    (left.kind === "builtin_musicbrainz" ||
      (right.kind === "http_plugin" &&
        left.installation_id === right.installation_id))
  );
}

function unavailable(
  status: Exclude<ArtistProfileAvailability["status"], "available">,
) {
  switch (status) {
    case "disabled":
      return "Metadata providers are disabled in your settings.";
    case "unsupported":
      return "This provider does not support artist profiles.";
    case "cancelled":
      return "The artist information request was cancelled.";
    case "unavailable":
      return "Artist information is unavailable. Try again.";
  }
}

export function useArtistGroupDiscography(
  account: string,
  group: ArtistIdentityGroupDetail["group"],
) {
  const scope = useRequestScope(
    JSON.stringify([
      account,
      group.id,
      group.provider,
      group.provider_artist_id,
      group.name,
    ]),
  );
  const owner = useMemo<Owner>(() => ({ pending: false }), [scope]);
  const [snapshot, setSnapshot] = useState(() => empty(owner));
  const current = snapshot.owner === owner ? snapshot : empty(owner);

  async function browse() {
    const request = scope.capture();
    if (!request.active() || owner.pending || current.ready) return;
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
    log.debug("artist_group_discography.started", { request_id: requestId });
    try {
      const ready = await Promise.race([
        (async () => {
          const associations = await artistReviewApi.associations(
            account,
            group.name.recording_id,
            controller.signal,
          );
          check();
          const association = associations.find(
            (item) => item.id === group.name.association_id,
          );
          if (
            !association ||
            association.account_id !== account ||
            association.recording_id !== group.name.recording_id ||
            association.artist_id !== group.name.artist_id ||
            association.provider_artist_id !== group.provider_artist_id ||
            !sameProvider(association.provider, group.provider)
          )
            throw new Error(CHANGED_ARTIST);
          const result = await artistReviewApi.profile(
            account,
            group.name.recording_id,
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
            result.profile.artist.id !== group.provider_artist_id
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
      record("artist_group_discography.finished", {
        request_id: requestId,
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  return { ...current, browse };
}
