import { useMemo } from "react";
import type { Track } from "../model";
import TrackRowActions from "./TrackRowActions";
import { useTrackActions } from "./TrackActionsProvider";

export default function PlayerTrackActions({
  track,
  queueActions = false,
  onNavigate,
}: {
  track: Track;
  queueActions?: boolean;
  onNavigate?: () => void;
}) {
  const actions = useTrackActions();
  const row = useMemo(
    () => ({ ...track, row_id: track.id, liked: track.liked ?? false }),
    [track],
  );
  if (!actions) return null;
  return (
    <TrackRowActions
      track={row}
      playlists={actions.playlists}
      mutate={actions.mutate}
      queueActions={queueActions}
      onNavigate={onNavigate}
    />
  );
}
