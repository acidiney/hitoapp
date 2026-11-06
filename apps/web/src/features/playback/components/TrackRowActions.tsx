import type { Schema } from "../../../shared/schema";
import { router } from "@inertiajs/react";
import type { ReactNode } from "react";
import { Heart } from "lucide-react";
import type { Mutate } from "../../../shared/hooks/useMutation";
import PlaylistEntryRemove from "./PlaylistEntryRemove";
import TrackRowMenu from "./TrackRowMenu";
import type { TrackRow } from "./TrackTable";

export default function TrackRowActions({
  track,
  playlists,
  playlist,
  mutate,
  onMutationAccepted,
  editing = false,
  children,
}: {
  editing?: boolean;
  children?: ReactNode;
  track: TrackRow;
  playlists: Schema<"Playlist">[];
  playlist?: Schema<"Playlist">;
  mutate: Mutate;
  onMutationAccepted?: (action: Schema<"Action">) => void;
}) {
  const entryId = track.entry_id;
  async function toggleLike() {
    const action: Schema<"Action"> = {
      kind: "like_set",
      recording_id: track.id,
      liked: !track.liked,
    };
    if (!(await mutate(action)).ok) return;
    if (onMutationAccepted) onMutationAccepted(action);
    else router.reload();
  }
  return (
    <div className="track-actions">
      <button
        className={`icon-button ${track.liked ? "selected" : ""}`.trim()}
        aria-label={`${track.liked ? "Unlike" : "Like"} ${track.title}`}
        aria-pressed={track.liked}
        onClick={() => void toggleLike()}
      >
        <Heart size={17} fill={track.liked ? "currentColor" : "none"} />
      </button>
      {children}
      {editing && playlist && entryId ? (
        <PlaylistEntryRemove
          entryId={entryId}
          title={track.title}
          playlist={playlist}
          mutate={mutate}
        />
      ) : (
        <TrackRowMenu
          track={track}
          playlists={playlists}
          playlist={playlist}
          mutate={mutate}
          onMutationAccepted={onMutationAccepted}
        />
      )}
    </div>
  );
}
