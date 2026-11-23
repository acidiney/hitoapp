import { useMemo } from "react";
import type { Track } from "../model";
import type { ActionMenuItem } from "../../../shared/ui/ActionMenu";
import { visitFromPlayer } from "../../../shared/ui/RouteLink";
import { log } from "../../../shared/log";
import TrackRowActions from "./TrackRowActions";
import { useTrackActions } from "./TrackActionsProvider";
import { usePlayerLinks, type PlayerLink } from "../hooks/usePlayerLinks";

function goToItems(
  artists: PlayerLink[],
  album: string | null,
  leave: (href: string) => void,
): ActionMenuItem[] {
  const items: ActionMenuItem[] = [];
  if (artists.length > 1)
    items.push({
      label: "Go to artist",
      items: artists.map((artist) => ({
        label: artist.label,
        onSelect: () => leave(artist.href),
      })),
    });
  else if (artists.length === 1)
    items.push({
      label: "Go to artist",
      onSelect: () => leave(artists[0].href),
    });
  if (album) items.push({ label: "Go to album", onSelect: () => leave(album) });
  return items;
}

export default function PlayerTrackActions({
  track,
  queueActions = false,
  liking = true,
  onNavigate,
}: {
  track: Track;
  queueActions?: boolean;
  liking?: boolean;
  onNavigate?: () => void;
}) {
  const actions = useTrackActions();
  const row = useMemo(
    () => ({ ...track, row_id: track.id, liked: track.liked ?? false }),
    [track],
  );
  const links = usePlayerLinks(actions?.account ?? "", actions ? track : null);
  if (!actions) return null;
  const leading = goToItems(links.artists, links.album, (href) => {
    log.info("player.go_to", { route: href.split("/")[1] ?? "" });
    onNavigate?.();
    visitFromPlayer(href);
  });
  return (
    <TrackRowActions
      track={row}
      playlists={actions.playlists}
      mutate={actions.mutate}
      queueActions={queueActions}
      liking={liking}
      onNavigate={onNavigate}
      leading={leading}
    />
  );
}
