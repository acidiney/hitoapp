import { useEffect, useState } from "react";
import { artistIdentityApi } from "../../artists/api/artistIdentityApi";
import { log } from "../../../shared/log";
import type { Track } from "../model";

const remembered = new Map<string, string>();
const rememberedLimit = 50;

function remember(key: string, href: string) {
  remembered.delete(key);
  remembered.set(key, href);
  if (remembered.size > rememberedLimit)
    remembered.delete(remembered.keys().next().value as string);
}

export function artistSearchLink(artist: string) {
  return `/search?q=${encodeURIComponent(artist)}`;
}

export function trackLink(track: Pick<Track, "id" | "release_id">) {
  return track.release_id
    ? `/albums/${encodeURIComponent(track.release_id)}`
    : `/recordings/${encodeURIComponent(track.id)}`;
}

export function useArtistLink(
  account: string,
  track: Pick<Track, "id" | "artist"> | null,
) {
  const key = track ? `${account}/${track.id}` : null;
  const [resolved, setResolved] = useState<{ key: string; href: string }>();
  useEffect(() => {
    if (!track || !key || remembered.has(key)) return;
    const controller = new AbortController();
    const started = performance.now();
    let outcome = "unmatched";
    artistIdentityApi
      .read(account, track.id, controller.signal)
      .then((choice) => {
        if (controller.signal.aborted) return;
        if (choice.state.status === "selected") {
          outcome = "matched";
          const href = `/artist-groups/${encodeURIComponent(choice.state.group_id)}`;
          remember(key, href);
          setResolved({ key, href });
        }
      })
      .catch(() => {
        outcome = controller.signal.aborted ? "cancelled" : "failed";
      })
      .finally(() =>
        log.debug("player.artist_link", {
          reason: outcome,
          duration_ms: Math.round(performance.now() - started),
        }),
      );
    return () => controller.abort();
  }, [account, key, track?.id]);
  if (!track || !key) return null;
  const matched =
    remembered.get(key) ?? (resolved?.key === key ? resolved.href : null);
  return matched ?? artistSearchLink(track.artist);
}
