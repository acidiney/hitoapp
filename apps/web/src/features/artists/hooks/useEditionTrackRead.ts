import type { Schema } from "../../../shared/schema";
import { useEffect, useMemo, useState } from "react";
import { errorMessage } from "../../../shared/format";
import { log } from "../../../shared/log";

import { createUuid } from "../../../shared/uuid";
import type { ArtistEditionContext } from "./useArtistEditions";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { associationAuthorizes } from "../model/associationAuthority";

const REQUEST_DEADLINE_MS = 20_000;
const MAX_MEDIA = 256;
const MAX_OCCURRENCES = 4_096;
type Owner = { pending: boolean };
type Available = {
  association_id: string;
  edition: Schema<"CatalogEditionTrackList">;
};
type Missing = {
  status:
    | "disabled"
    | "unsupported"
    | "unavailable"
    | "cancelled"
    | "needs_choice";
};
export type EditionRead<T extends Available> =
  | ({ status: "available" } & T)
  | Missing;
type State<T extends Available> = {
  owner: Owner;
  result: T | null;
  pending: boolean;
  error: string;
};

function empty<T extends Available>(owner: Owner): State<T> {
  return { owner, result: null, pending: false, error: "" };
}

function unavailable(status: Missing["status"]) {
  switch (status) {
    case "disabled":
      return "Metadata providers are disabled in your settings.";
    case "unsupported":
      return "This provider does not support complete edition track lists.";
    case "unavailable":
      return "This edition's track list is unavailable. Try again.";
    case "cancelled":
      return "The edition track list request was cancelled.";
    case "needs_choice":
      return "Choose this edition again.";
  }
}

function occurrenceCount(edition: Schema<"CatalogEditionTrackList">) {
  if (edition.media.length === 0 || edition.media.length > MAX_MEDIA)
    throw new Error("The provider returned an unsupported number of discs.");
  const known = new Set<string>();
  let count = 0;
  let previousMedium = 0;
  for (const medium of edition.media) {
    if (medium.position <= previousMedium)
      throw new Error("The provider returned invalid disc order.");
    previousMedium = medium.position;
    if (medium.tracks.length > MAX_OCCURRENCES - count)
      throw new Error("The provider returned too many track occurrences.");
    count += medium.tracks.length;
    let previousTrack = -1;
    for (const track of medium.tracks) {
      if (track.position <= previousTrack || known.has(track.id))
        throw new Error(
          "The provider returned invalid track occurrence order.",
        );
      previousTrack = track.position;
      known.add(track.id);
    }
  }
  return count;
}

export function useEditionTrackRead<T extends Available>(
  context: ArtistEditionContext,
  chosen: Schema<"CatalogEditionSummary"> | null,
  target: unknown,
  read: (signal: AbortSignal) => Promise<EditionRead<T>>,
) {
  const { association, releaseGroupId } = context;
  const scope = useRequestScope(JSON.stringify([context, chosen, target]));
  const owner = useMemo<Owner>(() => ({ pending: false }), [scope]);
  const [snapshot, setSnapshot] = useState(() => empty<T>(owner));
  const current = snapshot.owner === owner ? snapshot : empty<T>(owner);

  async function load() {
    const request = scope.capture();
    if (
      !chosen ||
      !request.active() ||
      owner.pending ||
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
        "The edition track list request was cancelled",
        "AbortError",
      );
      controller.abort(error);
      stop(error);
    };
    request.signal.addEventListener("abort", cancel, { once: true });
    const timer = setTimeout(() => {
      timedOut = true;
      const error = new Error(
        "This edition track list request timed out. Try again.",
      );
      controller.abort(error);
      stop(error);
    }, REQUEST_DEADLINE_MS);
    if (request.signal.aborted) cancel();
    setSnapshot({ ...empty<T>(owner), pending: true });
    log.debug("artist_edition_track_list.started", { request_id: requestId });
    try {
      const result = await Promise.race([read(controller.signal), stopped]);
      if (!request.active()) return;
      if (timedOut)
        throw new Error(
          "This edition track list request timed out. Try again.",
        );
      if (result.status !== "available") {
        outcome = result.status;
        throw new Error(unavailable(result.status));
      }
      if (
        result.association_id !== association.id ||
        result.edition.release_group_id !== releaseGroupId ||
        result.edition.id !== chosen.id
      )
        throw new Error(
          "This track list did not belong to the chosen edition and saved artist review. Browse again.",
        );
      count = occurrenceCount(result.edition);
      setSnapshot({
        owner,
        result: result as T,
        pending: false,
        error: "",
      });
      outcome = "accepted";
    } catch (error) {
      if (request.active()) {
        if (outcome === "cancelled")
          outcome = timedOut ? "timed_out" : "failed";
        setSnapshot({
          ...empty<T>(owner),
          error: timedOut
            ? "This edition track list request timed out. Try again."
            : errorMessage(error),
        });
      }
    } finally {
      clearTimeout(timer);
      request.signal.removeEventListener("abort", cancel);
      owner.pending = false;
      const record =
        outcome === "failed" || outcome === "timed_out" ? log.warn : log.debug;
      record("artist_edition_track_list.finished", {
        request_id: requestId,
        reason: outcome,
        count,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  useEffect(() => {
    if (!chosen) return;
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
