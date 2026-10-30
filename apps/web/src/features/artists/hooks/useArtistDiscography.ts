import type { Schema } from "../../../shared/schema";
import { useEffect, useMemo, useRef, useState } from "react";
import { artistDiscographyApi } from "../api/artistDiscographyApi";
import { errorMessage } from "../../../shared/format";
import { HttpError } from "../../../shared/http";
import { log } from "../../../shared/log";

import { createUuid } from "../../../shared/uuid";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";

const PAGE_LIMIT = 20;
const MAX_PAGES = 50;
const REQUEST_DEADLINE_MS = 20_000;

type Owner = { pending: boolean };
type State = {
  owner: Owner;
  opened: boolean;
  groups: Schema<"CatalogReleaseGroup">[];
  continuation: string | null;
  pages: number;
  pending: boolean;
  limited: boolean;
  restartRequired: boolean;
  error: string;
};

function empty(owner: Owner): State {
  return {
    owner,
    opened: false,
    groups: [],
    continuation: null,
    pages: 0,
    pending: false,
    limited: false,
    restartRequired: false,
    error: "",
  };
}

export const AUTO_RETRY_DELAY_MS = 800;

export function discographyUnavailable(
  status: Exclude<
    Schema<"ArtistDiscographyAvailability">["status"],
    "available" | "type_filter_unsupported"
  >,
) {
  switch (status) {
    case "disabled":
      return "Metadata providers are disabled in your settings.";
    case "unsupported":
      return "This provider does not support artist discographies.";
    case "unavailable":
      return "This discography is unavailable. Try again.";
    case "cancelled":
      return "The discography request was cancelled.";
  }
}

export function useArtistDiscography(
  account: string,
  recording: string,
  association: Schema<"ArtistAssociation">,
  profile: Schema<"CatalogArtistProfile">,
  browseOnMount = false,
) {
  const scope = useRequestScope(
    JSON.stringify([account, recording, association, profile]),
  );
  const owner = useMemo<Owner>(() => ({ pending: false }), [scope]);
  const [snapshot, setSnapshot] = useState(() => empty(owner));
  const current = snapshot.owner === owner ? snapshot : empty(owner);
  const lastOutcome = useRef("");
  const [retrying, setRetrying] = useState(false);

  async function load(continuation: string | null, restarting = false) {
    const request = scope.capture();
    if (
      !request.active() ||
      owner.pending ||
      (!restarting && (current.limited || current.pages >= MAX_PAGES)) ||
      association.account_id !== account ||
      association.recording_id !== recording ||
      association.provider_artist_id !== profile.artist.id
    )
      return;
    owner.pending = true;
    const requestId = createUuid();
    const started = performance.now();
    let outcome = "cancelled";
    let count = 0;
    let timedOut = false;
    const controller = new AbortController();
    let stop!: (error: Error) => void;
    const stopped = new Promise<never>((_, reject) => {
      stop = reject;
    });
    const cancel = () => {
      const error = new DOMException(
        "The discography request was cancelled",
        "AbortError",
      );
      controller.abort(error);
      stop(error);
    };
    request.signal.addEventListener("abort", cancel, { once: true });
    const timer = setTimeout(() => {
      timedOut = true;
      const error = new Error("This discography request timed out. Try again.");
      controller.abort(error);
      stop(error);
    }, REQUEST_DEADLINE_MS);
    if (request.signal.aborted) cancel();
    setSnapshot((value) => ({
      ...(value.owner === owner ? value : empty(owner)),
      opened: true,
      pending: true,
      error: "",
    }));
    log.debug("artist_discography.started", {
      action: restarting
        ? "restart"
        : continuation === null
          ? "browse"
          : "more",
      request_id: requestId,
    });
    try {
      const result = await Promise.race([
        artistDiscographyApi.read(
          account,
          recording,
          association.id,
          { limit: PAGE_LIMIT, continuation },
          controller.signal,
        ),
        stopped,
      ]);
      if (!request.active()) return;
      if (timedOut)
        throw new Error("This discography request timed out. Try again.");
      if (result.status === "type_filter_unsupported")
        throw new Error(
          "The provider returned an unexpected discography status.",
        );
      if (result.status !== "available")
        throw new Error(discographyUnavailable(result.status));
      if (
        result.association_id !== association.id ||
        result.page.artist_id !== profile.artist.id
      )
        throw new Error(
          "The discography did not belong to this saved artist review. Load the review again.",
        );
      if (result.page.items.length > PAGE_LIMIT)
        throw new Error("The provider returned too many releases. Try again.");
      count = result.page.items.length;
      setSnapshot((value) => {
        if (!request.active() || value.owner !== owner) return value;
        const previous = restarting ? [] : value.groups;
        const known = new Set(previous.map((group) => group.id));
        const added = result.page.items.filter((group) => {
          if (known.has(group.id)) return false;
          known.add(group.id);
          return true;
        });
        const pages = restarting ? 1 : value.pages + 1;
        const limited = pages >= MAX_PAGES && !!result.continuation;
        return {
          ...value,
          groups: [...previous, ...added],
          continuation: limited ? null : (result.continuation ?? null),
          pages,
          limited,
          error: "",
          restartRequired: false,
        };
      });
      outcome = "accepted";
    } catch (error) {
      if (request.active()) {
        outcome = timedOut ? "timed_out" : "failed";
        setSnapshot((value) =>
          request.active() && value.owner === owner
            ? {
                ...value,
                error: timedOut
                  ? "This discography request timed out. Try again."
                  : errorMessage(error),
                restartRequired:
                  value.restartRequired ||
                  (!timedOut &&
                    error instanceof HttpError &&
                    error.status === 409),
              }
            : value,
        );
      }
    } finally {
      clearTimeout(timer);
      request.signal.removeEventListener("abort", cancel);
      owner.pending = false;
      lastOutcome.current = outcome;
      if (request.active())
        setSnapshot((value) =>
          request.active() && value.owner === owner
            ? { ...value, pending: false }
            : value,
        );
      const record =
        outcome === "failed" || outcome === "timed_out" ? log.warn : log.debug;
      record("artist_discography.finished", {
        request_id: requestId,
        reason: outcome,
        count,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  useEffect(() => {
    if (!browseOnMount) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    queueMicrotask(async () => {
      if (!active) return;
      await load(null);
      if (!active || lastOutcome.current === "accepted") return;
      if (
        lastOutcome.current !== "failed" &&
        lastOutcome.current !== "timed_out"
      )
        return;
      setRetrying(true);
      await new Promise<void>((resolve) => {
        timer = setTimeout(resolve, AUTO_RETRY_DELAY_MS);
      });
      if (!active) return;
      setRetrying(false);
      await load(null, true);
    });
    return () => {
      active = false;
      setRetrying(false);
      clearTimeout(timer);
    };
  }, [browseOnMount, owner, scope]);

  return {
    ...current,
    retrying,
    browse: () => (!current.opened ? load(null) : Promise.resolve()),
    more: () =>
      current.continuation && !current.restartRequired
        ? load(current.continuation)
        : Promise.resolve(),
    retry: () =>
      !current.restartRequired ? load(current.continuation) : Promise.resolve(),
    restart: () =>
      current.restartRequired ? load(null, true) : Promise.resolve(),
  };
}
