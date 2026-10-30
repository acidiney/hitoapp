import { QueueStop } from "../QueueFeed";
import { MAX_QUEUE_ENTRIES, type QueueSupply, type Track } from "../model";

export const LISTING_PAGE = 200;

export type TrackListing = {
  total: number;
  offset: number;
  fetch(
    offset: number,
    limit: number,
    signal: AbortSignal,
  ): Promise<{ total: number; items: Track[] }>;
};

export type ListingPage = { total: number; offset: number };

export function trackListing(
  page: ListingPage,
  fetchPage: TrackListing["fetch"],
): TrackListing {
  return { total: page.total, offset: page.offset, fetch: fetchPage };
}

export function listingSupply(
  listing: TrackListing,
  from: number,
  queued = from,
): QueueSupply {
  let next = from;
  let end = Math.min(listing.total, from + MAX_QUEUE_ENTRIES - queued);
  return {
    remaining: () => Math.max(0, end - next),
    async next(signal) {
      signal.throwIfAborted();
      const page = await listing.fetch(next, LISTING_PAGE, signal);
      signal.throwIfAborted();
      if (!page.items.length) {
        end = next;
        return [];
      }
      if (page.total !== listing.total)
        throw new QueueStop(
          "The list changed while loading. Play it again to include the rest.",
        );
      next += page.items.length;
      return page.items;
    },
  };
}
