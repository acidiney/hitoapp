import type { Schema } from "../../../shared/schema";
import { useEffect, useMemo, useRef, useState } from "react";
import { artistReviewApi } from "../api/artistReviewApi";
import { errorMessage } from "../../../shared/format";
import { log } from "../../../shared/log";

import { createUuid } from "../../../shared/uuid";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { groupAssociation } from "../model/associationAuthority";

const REQUEST_DEADLINE_MS = 20_000;
export const AUTO_RETRY_DELAY_MS = 800;
const CHANGED_ARTIST =
  "This chosen artist has changed. Reload the page to browse its discography.";

class FinalError extends Error {}

type IdentityGroup = Schema<"ArtistIdentityGroupDetail">["group"];
type Owner = { pending: boolean };
type State<T> = {
  owner: Owner;
  ready: T | null;
  pending: boolean;
  error: string;
};
type Resolve<T> = (
  association: Schema<"ArtistAssociation">,
  signal: AbortSignal,
  report: (outcome: string) => void,
) => Promise<T>;

function empty<T>(owner: Owner): State<T> {
  return { owner, ready: null, pending: false, error: "" };
}

function unavailable(
  status: Exclude<Schema<"ArtistProfileAvailability">["status"], "available">,
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

function useGroupAnchor<T>(
  account: string,
  group: IdentityGroup,
  event: string,
  resolve: Resolve<T>,
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
  const [snapshot, setSnapshot] = useState(() => empty<T>(owner));
  const current = snapshot.owner === owner ? snapshot : empty<T>(owner);
  const [retrying, setRetrying] = useState(false);
  const retryable = useRef(true);
  const resolver = useRef(resolve);
  resolver.current = resolve;

  async function browse(): Promise<boolean> {
    const request = scope.capture();
    if (!request.active() || owner.pending || current.ready) return false;
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
    setSnapshot({ ...empty<T>(owner), pending: true });
    log.debug(`${event}.started`, { request_id: requestId });
    try {
      const ready = await Promise.race([
        (async () => {
          const associations = await artistReviewApi.associations(
            account,
            group.name.recording_id,
            controller.signal,
          );
          check();
          const association = groupAssociation(associations, account, group);
          if (!association) throw new FinalError(CHANGED_ARTIST);
          const result = await resolver.current(
            association,
            controller.signal,
            (status) => {
              outcome = status;
            },
          );
          check();
          return result;
        })(),
        stopped,
      ]);
      check();
      setSnapshot({ owner, ready, pending: false, error: "" });
      outcome = "accepted";
      return true;
    } catch (error) {
      if (request.active()) {
        if (outcome === "cancelled")
          outcome = timedOut ? "timed_out" : "failed";
        setSnapshot({
          ...empty<T>(owner),
          error: timedOut
            ? "Artist information timed out. Try again."
            : errorMessage(error),
        });
      }
      retryable.current = !(error instanceof FinalError);
      return false;
    } finally {
      clearTimeout(timer);
      request.signal.removeEventListener("abort", cancel);
      owner.pending = false;
      const record =
        outcome === "failed" || outcome === "timed_out" ? log.warn : log.debug;
      record(`${event}.finished`, {
        request_id: requestId,
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  const latest = useRef(browse);
  latest.current = browse;

  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    queueMicrotask(async () => {
      if (!active || (await latest.current()) || !active || !retryable.current)
        return;
      setRetrying(true);
      await new Promise<void>((resolve) => {
        timer = setTimeout(resolve, AUTO_RETRY_DELAY_MS);
      });
      if (!active) return;
      setRetrying(false);
      await latest.current();
    });
    return () => {
      active = false;
      setRetrying(false);
      clearTimeout(timer);
    };
  }, [owner, scope]);

  return { ...current, retrying, browse };
}

export function useArtistGroupDiscography(
  account: string,
  group: IdentityGroup,
) {
  return useGroupAnchor(
    account,
    group,
    "artist_group_discography",
    async (association, signal, report) => {
      const result = await artistReviewApi.profile(
        account,
        group.name.recording_id,
        association.id,
        signal,
      );
      if (result.status !== "available") {
        report(result.status);
        throw result.status === "unavailable"
          ? new Error(unavailable(result.status))
          : new FinalError(unavailable(result.status));
      }
      if (
        result.association_id !== association.id ||
        result.profile.artist.id !== group.provider_artist_id
      )
        throw new FinalError(CHANGED_ARTIST);
      return { association, profile: result.profile };
    },
  );
}

export function useCatalogAlbumAnchor(account: string, group: IdentityGroup) {
  return useGroupAnchor(
    account,
    group,
    "catalog_album_anchor",
    async (association) => ({ association }),
  );
}
