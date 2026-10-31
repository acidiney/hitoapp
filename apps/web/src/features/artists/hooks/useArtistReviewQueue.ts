import { router } from "@inertiajs/react";
import { useCallback, useEffect, useState } from "react";
import type { Schema } from "../../../shared/schema";
import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { artistMatchingApi } from "../api/artistMatchingApi";

/** A confirmation applies at most 200 tracks; a credit this long is finished in a few steps. */
const MAX_CONFIRM_STEPS = 10;

type Queue = Schema<"ArtistReviewQueuePage">;

type View = {
  status: "loading" | "ready" | "failed";
  queue: Queue | null;
  /** The credit a decision is being saved for. */
  pending: string | null;
  error: string;
};

/**
 * The credits the automatic walk could not identify. It reads the stored queue (no provider is
 * asked), saves one decision at a time and reads the queue again afterwards. Confirming also
 * refreshes the artist lists so the artist appears among the chosen artists.
 */
export function useArtistReviewQueue(account: string, refreshKey: string) {
  const scope = useRequestScope(account);
  const [view, setView] = useState<View>({
    status: "loading",
    queue: null,
    pending: null,
    error: "",
  });

  const read = useCallback(async () => {
    const request = scope.capture();
    try {
      const queue = await artistMatchingApi.reviewQueue(
        account,
        request.signal,
      );
      if (!request.active()) return;
      setView((current) => ({ ...current, status: "ready", queue }));
      log.debug("artist_review_queue.loaded", { count: queue.items.length });
    } catch {
      if (request.active()) {
        log.debug("artist_review_queue.unavailable");
        setView((current) => ({ ...current, status: "failed", queue: null }));
      }
    }
  }, [account, scope]);

  useEffect(() => {
    void read();
  }, [read, refreshKey]);

  const decide = useCallback(
    async (
      credit: string,
      save: (signal: AbortSignal, saved: () => void) => Promise<void>,
      reason: string,
    ) => {
      const request = scope.capture();
      setView((current) => ({ ...current, pending: credit, error: "" }));
      const started = performance.now();
      let changedArtists = false;
      try {
        await save(request.signal, () => {
          changedArtists = reason === "confirmed";
        });
        if (!request.active()) return;
        log.debug(`artist_review_queue.${reason}`, {
          duration_ms: Math.round(performance.now() - started),
        });
        if (changedArtists)
          router.reload({ only: ["artists", "identity_groups"] });
        await read();
      } catch {
        if (!request.active()) return;
        log.debug("artist_review_queue.decision_failed", { reason });
        if (changedArtists) {
          router.reload({ only: ["artists", "identity_groups"] });
          await read();
        }
        setView((current) => ({
          ...current,
          error: "This decision could not be saved. Try again.",
        }));
      } finally {
        if (request.active())
          setView((current) => ({ ...current, pending: null }));
      }
    },
    [read, scope],
  );

  const confirm = useCallback(
    (credit: string, artist: string) =>
      decide(
        credit,
        async (signal, saved) => {
          let after: string | null = null;
          for (let step = 0; step < MAX_CONFIRM_STEPS; step += 1) {
            const receipt = await artistMatchingApi.confirmCandidate(
              account,
              credit,
              { request_id: createUuid(), provider_artist_id: artist, after },
              signal,
            );
            saved();
            if (!receipt.next_after) break;
            after = receipt.next_after;
          }
        },
        "confirmed",
      ),
    [account, decide],
  );

  const dismiss = useCallback(
    (credit: string, artist: string | null) =>
      decide(
        credit,
        async (signal) => {
          await artistMatchingApi.dismissCandidates(
            account,
            credit,
            artist === null ? {} : { provider_artist_id: artist },
            signal,
          );
        },
        "dismissed",
      ),
    [account, decide],
  );

  return { ...view, confirm, dismiss };
}
