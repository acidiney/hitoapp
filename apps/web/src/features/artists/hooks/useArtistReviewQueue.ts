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
  /** Credits whose candidates the account dismissed; null when they could not be read. */
  dismissed: Queue | null;
  dismissedFailed: boolean;
  /** The credit a decision is being saved for. */
  pending: string | null;
  error: string;
  restoreError: string;
};

/**
 * The credits the automatic walk could not identify, and the candidates the account dismissed. It
 * reads the stored lists (no provider is asked), saves one decision at a time and reads both lists
 * again afterwards. Confirming also refreshes the artist lists so the artist appears among the
 * chosen artists.
 */
export function useArtistReviewQueue(account: string, refreshKey: string) {
  const scope = useRequestScope(account);
  const [view, setView] = useState<View>({
    status: "loading",
    queue: null,
    dismissed: null,
    dismissedFailed: false,
    pending: null,
    error: "",
    restoreError: "",
  });

  const read = useCallback(async () => {
    const request = scope.capture();
    const dismissedRead = artistMatchingApi
      .dismissedReviews(account, request.signal)
      .catch(() => null);
    try {
      const queue = await artistMatchingApi.reviewQueue(
        account,
        request.signal,
      );
      const dismissed = await dismissedRead;
      if (!request.active()) return;
      setView((current) => ({
        ...current,
        status: "ready",
        queue,
        dismissed,
        dismissedFailed: dismissed === null,
      }));
      log.debug("artist_review_queue.loaded", {
        count: queue.items.length,
      });
    } catch {
      if (request.active()) {
        log.debug("artist_review_queue.unavailable");
        setView((current) => ({
          ...current,
          status: "failed",
          queue: null,
          dismissed: null,
        }));
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
      setView((current) => ({
        ...current,
        pending: credit,
        error: "",
        restoreError: "",
      }));
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
        setView((current) =>
          reason === "restored"
            ? {
                ...current,
                restoreError:
                  "These suggestions could not be restored. Try again.",
              }
            : {
                ...current,
                error: "This decision could not be saved. Try again.",
              },
        );
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

  const more = useCallback(
    async (list: "queue" | "dismissed") => {
      const request = scope.capture();
      const have = view[list]?.items.length ?? 0;
      try {
        const next = await (list === "queue"
          ? artistMatchingApi.reviewQueue(account, request.signal, have)
          : artistMatchingApi.dismissedReviews(account, request.signal, have));
        if (!request.active()) return;
        setView((current) => {
          const base = current[list];
          if (!base) return current;
          return {
            ...current,
            [list]: { ...next, items: [...base.items, ...next.items] },
          };
        });
      } catch {
        log.debug("artist_review_queue.more_failed", { reason: list });
      }
    },
    [account, scope, view],
  );

  const restore = useCallback(
    (credit: string, artist: string | null) =>
      decide(
        credit,
        async (signal) => {
          await artistMatchingApi.restoreCandidates(
            account,
            credit,
            artist === null ? {} : { provider_artist_id: artist },
            signal,
          );
        },
        "restored",
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

  return { ...view, confirm, dismiss, restore, more };
}
