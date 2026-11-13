import type { Schema } from "../../../shared/schema";
import { useEffect, useMemo, useState } from "react";
import { artistEditionApi } from "../api/artistEditionApi";
import { errorMessage } from "../../../shared/format";
import { HttpError } from "../../../shared/http";
import { log } from "../../../shared/log";

import { createUuid } from "../../../shared/uuid";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { associationAuthorizes } from "../model/associationAuthority";

const PAGE_LIMIT = 20;
const MAX_PAGES = 50;
const REQUEST_DEADLINE_MS = 20_000;

export type ArtistEditionContext = {
  account: string;
  recording: string;
  association: Schema<"ArtistAssociation">;
  providerArtistId: string;
  releaseGroupId: string;
  release?: string;
  onAlbumEdition?: (edition: Schema<"SavedAlbumEdition"> | null) => void;
};
type Owner = { pending: boolean };
type State = {
  owner: Owner;
  opened: boolean;
  editions: Schema<"CatalogEditionSummary">[];
  chosen: Schema<"CatalogEditionSummary"> | null;
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
    editions: [],
    chosen: null,
    continuation: null,
    pages: 0,
    pending: false,
    limited: false,
    restartRequired: false,
    error: "",
  };
}

function unavailable(
  status: Exclude<
    Schema<"ReleaseGroupEditionsAvailability">["status"],
    "available"
  >,
) {
  switch (status) {
    case "disabled":
      return "Metadata providers are disabled in your settings.";
    case "unsupported":
      return "This provider does not support edition browsing.";
    case "unavailable":
      return "Editions are unavailable. Try again.";
    case "cancelled":
      return "The editions request was cancelled.";
  }
}

export function useArtistEditions(
  context: ArtistEditionContext,
  browseOnMount = false,
) {
  const { account, recording, association, releaseGroupId } = context;
  const scope = useRequestScope(JSON.stringify(context));
  const owner = useMemo<Owner>(() => ({ pending: false }), [scope]);
  const [snapshot, setSnapshot] = useState(() => empty(owner));
  const current = snapshot.owner === owner ? snapshot : empty(owner);

  async function load(continuation: string | null, restarting = false) {
    const request = scope.capture();
    if (
      !request.active() ||
      owner.pending ||
      (!restarting && (current.limited || current.pages >= MAX_PAGES)) ||
      !associationAuthorizes(context)
    )
      return;
    owner.pending = true;
    const requestId = createUuid();
    const started = performance.now();
    const controller = new AbortController();
    let stop!: (error: Error) => void;
    const stopped = new Promise<never>((_, reject) => {
      stop = reject;
    });
    let timedOut = false;
    let outcome = "cancelled";
    let count = 0;
    const cancel = () => {
      const error = new DOMException(
        "The editions request was cancelled",
        "AbortError",
      );
      controller.abort(error);
      stop(error);
    };
    request.signal.addEventListener("abort", cancel, { once: true });
    const timer = setTimeout(() => {
      timedOut = true;
      const error = new Error("This editions request timed out. Try again.");
      controller.abort(error);
      stop(error);
    }, REQUEST_DEADLINE_MS);
    if (request.signal.aborted) cancel();
    setSnapshot((value) => ({
      ...(value.owner === owner ? value : empty(owner)),
      ...(restarting ? { chosen: null } : {}),
      opened: true,
      pending: true,
      error: "",
    }));
    log.debug("artist_editions.started", {
      request_id: requestId,
      action: restarting
        ? "restart"
        : continuation === null
          ? "browse"
          : "more",
    });
    try {
      const result = await Promise.race([
        artistEditionApi.editions(
          account,
          recording,
          association.id,
          { release_group_id: releaseGroupId, limit: PAGE_LIMIT, continuation },
          controller.signal,
          restarting,
        ),
        stopped,
      ]);
      if (!request.active()) return;
      if (timedOut)
        throw new Error("This editions request timed out. Try again.");
      if (result.status !== "available") {
        outcome = result.status;
        throw new Error(unavailable(result.status));
      }
      if (
        result.association_id !== association.id ||
        result.page.release_group_id !== releaseGroupId
      )
        throw new Error(
          "These editions did not belong to this saved artist review and release group. Browse again.",
        );
      if (result.page.items.length > PAGE_LIMIT)
        throw new Error("The provider returned too many editions. Try again.");
      count = result.page.items.length;
      setSnapshot((value) => {
        if (!request.active() || value.owner !== owner) return value;
        const previous = restarting ? [] : value.editions;
        const known = new Set(previous.map((edition) => edition.id));
        const added = result.page.items.filter((edition) => {
          if (known.has(edition.id)) return false;
          known.add(edition.id);
          return true;
        });
        const pages = restarting ? 1 : value.pages + 1;
        const limited = pages >= MAX_PAGES && !!result.continuation;
        return {
          ...value,
          editions: [...previous, ...added],
          continuation: limited ? null : (result.continuation ?? null),
          pages,
          limited,
          restartRequired: false,
          error: "",
        };
      });
      outcome = "accepted";
    } catch (error) {
      if (request.active()) {
        if (outcome === "cancelled")
          outcome = timedOut ? "timed_out" : "failed";
        setSnapshot((value) =>
          request.active() && value.owner === owner
            ? {
                ...value,
                error: timedOut
                  ? "This editions request timed out. Try again."
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
      if (request.active())
        setSnapshot((value) =>
          request.active() && value.owner === owner
            ? { ...value, pending: false }
            : value,
        );
      const record =
        outcome === "failed" || outcome === "timed_out" ? log.warn : log.debug;
      record("artist_editions.finished", {
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
    queueMicrotask(() => {
      if (active) void load(null);
    });
    return () => {
      active = false;
    };
  }, [browseOnMount, owner, scope]);

  return {
    ...current,
    browse: () => (!current.opened ? load(null) : Promise.resolve()),
    more: () =>
      current.continuation && !current.restartRequired
        ? load(current.continuation)
        : Promise.resolve(),
    retry: () =>
      !current.restartRequired ? load(current.continuation) : Promise.resolve(),
    restart: () =>
      current.error && (current.pages > 0 || current.restartRequired)
        ? load(null, true)
        : Promise.resolve(),
    choose: (id: string) => {
      const chosen = current.editions.find((edition) => edition.id === id);
      if (!chosen || current.pending) return;
      setSnapshot((value) =>
        value.owner === owner ? { ...value, chosen } : value,
      );
    },
  };
}
