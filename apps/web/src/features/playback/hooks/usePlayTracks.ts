import { usePlayer } from "../components/PlaybackProvider";
import {
  LISTING_PAGE,
  listingSupply,
  type TrackListing,
} from "../api/trackListing";
import type { PlaybackStart, Track } from "../model";
import { log } from "../../../shared/log";

export function usePlayTracks(tracks: Track[], listing?: TrackListing) {
  const player = usePlayer();
  const after = (loaded: number, from: number) =>
    listing && listing.total > from
      ? listingSupply(listing, from, loaded)
      : undefined;
  return {
    playFrom(index: number, start?: PlaybackStart) {
      const more = after(tracks.length, (listing?.offset ?? 0) + tracks.length);
      return more
        ? player.playQueue(tracks, index, start, more)
        : player.playQueue(tracks, index, start);
    },
    async playAll() {
      const more = after(tracks.length, (listing?.offset ?? 0) + tracks.length);
      if (!listing || listing.offset === 0)
        return more
          ? player.playQueue(tracks, 0, undefined, more)
          : player.playQueue(tracks);
      const select = player.prepareSelection();
      try {
        const first = await listing.fetch(
          0,
          LISTING_PAGE,
          new AbortController().signal,
        );
        if (first.items.length)
          return void (await select({
            queue: first.items,
            index: 0,
            more: after(first.items.length, first.items.length),
          }));
      } catch {
        log.warn("queue.first_page_failed", { reason: "fetch" });
      }
      await select({ queue: tracks, index: 0, more });
    },
  };
}
