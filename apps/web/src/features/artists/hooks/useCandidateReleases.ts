import { useEffect, useRef, useState } from "react";
import type { Schema } from "../../../shared/schema";
import { artistReviewApi } from "../api/artistReviewApi";
import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";

type Releases =
  | { phase: "idle" }
  | { phase: "loading" }
  | { phase: "listed"; titles: string[] }
  | { phase: "failed"; message: string };

type Unlisted = Exclude<
  Schema<"ArtistCandidateReleasesAvailability">["status"],
  "available"
>;

const unlistedMessages: Record<Unlisted, string> = {
  disabled: "Metadata providers are disabled in your settings.",
  unsupported: "This provider does not list releases.",
  unavailable: "Releases are unavailable. Try again.",
  cancelled: "Releases are unavailable. Try again.",
};

export function useCandidateReleases(
  account: string,
  recording: string,
  context: Schema<"ArtistReviewContext">,
  artist: string,
) {
  const [releases, setReleases] = useState<Releases>({ phase: "idle" });
  const controller = useRef<AbortController | null>(null);

  useEffect(
    () => () => {
      controller.current?.abort();
    },
    [],
  );

  async function load() {
    if (controller.current && releases.phase === "loading") return;
    controller.current?.abort();
    const request = new AbortController();
    controller.current = request;
    const requestId = createUuid();
    const started = performance.now();
    let outcome = "cancelled";
    setReleases({ phase: "loading" });
    log.debug("artist_review.started", {
      action: "releases",
      request_id: requestId,
    });
    try {
      const result = await artistReviewApi.releases(
        account,
        recording,
        { context, provider_artist_id: artist },
        request.signal,
      );
      if (request.signal.aborted) return;
      outcome = result.status;
      setReleases(
        result.status === "available"
          ? { phase: "listed", titles: result.titles }
          : { phase: "failed", message: unlistedMessages[result.status] },
      );
    } catch {
      if (request.signal.aborted) return;
      outcome = "failed";
      setReleases({
        phase: "failed",
        message: unlistedMessages.unavailable,
      });
    } finally {
      log.debug("artist_review.finished", {
        action: "releases",
        request_id: requestId,
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  return { releases, load };
}
