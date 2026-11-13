import { useCallback, useEffect, useState } from "react";
import type { Schema } from "../../../shared/schema";
import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { artistMatchingApi } from "../api/artistMatchingApi";

const MAX_QUEUE_PAGES = 4;
const MAX_CONFIRM_STEPS = 10;

export type CreditTailOffer = {
  group: Schema<"ArtistIdentityGroup">;
  providerArtistId: string;
  undecidedTracks: number;
};

type View = {
  offer: CreditTailOffer | null;
  pending: boolean;
  error: string;
};

const idle: View = { offer: null, pending: false, error: "" };

function offerIn(
  page: Schema<"ArtistReviewQueuePage">,
  credit: string,
  chosen: Schema<"ArtistMatchChosen">[],
): CreditTailOffer | null | undefined {
  const item = page.items.find((entry) => entry.artist_id === credit);
  if (!item) return undefined;
  const candidates = [...item.candidates].sort(
    (left, right) =>
      Number(right.origin === "matched_on_other_tracks") -
      Number(left.origin === "matched_on_other_tracks"),
  );
  for (const candidate of candidates) {
    const match = chosen.find(
      (entry) =>
        entry.automatic_tracks > 0 &&
        entry.group.provider.kind === "builtin_musicbrainz" &&
        entry.group.provider_artist_id === candidate.provider_artist_id,
    );
    if (match)
      return {
        group: match.group,
        providerArtistId: candidate.provider_artist_id,
        undecidedTracks: item.undecided_tracks,
      };
  }
  return null;
}

export function useCreditTailOffer(
  account: string,
  credit: string,
  chosen: Schema<"ArtistMatchChosen">[] | null,
  onApplied: () => void,
) {
  const scope = useRequestScope(`${account}:${credit}`);
  const [view, setView] = useState<View>(idle);
  const wanted = chosen !== null && chosen.length > 0;

  useEffect(() => {
    if (!wanted || !chosen) {
      setView(idle);
      return;
    }
    const request = scope.capture();
    const started = performance.now();
    void (async () => {
      let found: CreditTailOffer | null = null;
      let pages = 0;
      try {
        let offset = 0;
        while (pages < MAX_QUEUE_PAGES) {
          const page = await artistMatchingApi.reviewQueue(
            account,
            request.signal,
            offset,
          );
          pages += 1;
          if (!Array.isArray(page?.items)) break;
          const result = offerIn(page, credit, chosen);
          if (result !== undefined) {
            found = result;
            break;
          }
          offset += page.items.length;
          if (page.items.length === 0 || offset >= page.total) break;
        }
      } catch {
        if (request.active()) log.debug("credit_tail_offer.unavailable");
      }
      if (!request.active()) return;
      setView((current) => ({ ...current, offer: found }));
      log.debug("credit_tail_offer.loaded", {
        enabled: found !== null,
        count: pages,
        duration_ms: Math.round(performance.now() - started),
      });
    })();
  }, [account, credit, chosen, wanted, scope]);

  const confirm = useCallback(async () => {
    const offer = view.offer;
    if (!offer || view.pending) return;
    const request = scope.capture();
    const started = performance.now();
    setView((current) => ({ ...current, pending: true, error: "" }));
    let applied = 0;
    let reason = "completed";
    try {
      let after: string | null = null;
      for (let step = 0; step < MAX_CONFIRM_STEPS; step += 1) {
        const receipt = await artistMatchingApi.confirmCandidate(
          account,
          credit,
          {
            request_id: createUuid(),
            provider_artist_id: offer.providerArtistId,
            after,
          },
          request.signal,
        );
        applied += receipt.applied;
        if (!receipt.next_after) break;
        after = receipt.next_after;
      }
      if (!request.active()) return;
      setView(idle);
    } catch {
      reason = "failed";
      if (!request.active()) return;
      setView((current) => ({
        ...current,
        pending: false,
        error:
          applied > 0
            ? "Hito stopped before every track was updated. The tracks already updated keep the artist."
            : "Hito could not use this artist for the other tracks. Try again.",
      }));
    } finally {
      log.info("credit_tail_offer.confirmed", {
        reason,
        count: applied,
        duration_ms: Math.round(performance.now() - started),
      });
    }
    onApplied();
  }, [account, credit, onApplied, scope, view.offer, view.pending]);

  return { ...view, confirm };
}
