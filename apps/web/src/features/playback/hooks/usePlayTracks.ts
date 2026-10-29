import { useEffect, useRef } from "react";
import { usePlayer } from "../components/PlaybackProvider";
import {
  LISTING_PAGE,
  listingSupply,
  type TrackListing,
} from "../api/trackListing";
import { QueueStop } from "../QueueFeed";
import type { PlaybackStart, QueueSupply, Track } from "../model";
import { log } from "../../../shared/log";

const PARTIAL =
  "The start of the list could not be loaded. Only this page was queued.";

export function usePlayTracks(tracks: Track[], listing?: TrackListing) {
  const player = usePlayer();
  const fetching = useRef<AbortController | null>(null);
  useEffect(
    () => () => {
      fetching.current?.abort();
      fetching.current = null;
    },
    [player],
  );
  const after = (loaded: number, from: number) =>
    listing && listing.total > from
      ? listingSupply(listing, from, loaded)
      : undefined;
  return {
    /** Starts from a random track and turns shuffle on only once playback was accepted. */
    async shuffleAll() {
      const from = !listing || listing.offset === 0;
      const index = from ? Math.floor(Math.random() * tracks.length) : 0;
      try {
        if (from) await this.playFrom(index);
        else await this.playAll();
      } catch {
        return;
      }
      if (!player.snapshot().shuffle) player.shuffle();
    },
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
      fetching.current?.abort();
      const controller = new AbortController();
      fetching.current = controller;
      const select = player.prepareSelection();
      let queue: Track[] = tracks;
      let supply: QueueSupply | undefined = more;
      try {
        const first = await listing.fetch(0, LISTING_PAGE, controller.signal);
        if (first.items.length) {
          queue = first.items;
          supply = after(first.items.length, first.items.length);
        }
      } catch {
        if (controller.signal.aborted) return;
        log.warn("queue.first_page_failed", { reason: "fetch" });
        supply = {
          remaining: () => 1,
          next: async () => {
            throw new QueueStop(PARTIAL);
          },
        };
      }
      if (controller.signal.aborted) return;
      await select({ queue, index: 0, more: supply });
    },
  };
}
