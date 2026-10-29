import { createUuid } from "../../../shared/uuid";
import { Link, router } from "@inertiajs/react";
import type { ReactNode } from "react";
import { Heart, Info, X } from "lucide-react";
import type { Action, Playlist } from "../../../shared/types";
import type { Mutate } from "../../../shared/hooks/useMutation";
import type { TrackRow } from "./TrackTable";

export function PlaylistEntryRemove({
  entryId,
  title,
  playlist,
  mutate,
}: {
  entryId: string;
  title: string;
  playlist: Playlist;
  mutate: Mutate;
}) {
  if (playlist.kind !== "local") return null;
  async function remove() {
    const result = await mutate({
      kind: "entry_remove",
      playlist_id: playlist.id,
      entry_id: entryId,
    });
    if (result.ok) router.reload();
  }
  return (
    <button
      className="icon-button"
      aria-label={`Remove ${title} from playlist`}
      onClick={() => void remove()}
    >
      <X size={16} />
    </button>
  );
}

export default function TrackActions({
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
  playlists: Playlist[];
  playlist?: Playlist;
  mutate: Mutate;
  onMutationAccepted?: () => void;
}) {
  const editable = playlists.filter((list) => list.kind === "local");
  const entryId = track.entry_id;
  async function update(action: Action) {
    if ((await mutate(action)).ok) {
      if (onMutationAccepted) onMutationAccepted();
      else router.reload();
    }
  }
  return (
    <div className="track-actions">
      <Link
        className="icon-button"
        href={`/recordings/${encodeURIComponent(track.id)}`}
        aria-label={`Details for ${track.title}`}
      >
        <Info size={17} />
      </Link>
      <button
        className={`icon-button ${track.liked ? "selected" : ""}`}
        aria-label={`${track.liked ? "Unlike" : "Like"} ${track.title}`}
        onClick={() =>
          void update({
            kind: "like_set",
            recording_id: track.id,
            liked: !track.liked,
          })
        }
      >
        <Heart size={17} fill={track.liked ? "currentColor" : "none"} />
      </button>
      {children}
      {playlist
        ? editing &&
          entryId && (
            <PlaylistEntryRemove
              entryId={entryId}
              title={track.title}
              playlist={playlist}
              mutate={mutate}
            />
          )
        : editable.length > 0 && (
            <select
              className="add-to-playlist"
              aria-label={`Add ${track.title} to playlist`}
              value=""
              onChange={(event) => {
                if (event.target.value)
                  void update({
                    kind: "entry_insert",
                    playlist_id: event.target.value,
                    entry_id: createUuid(),
                    recording_id: track.id,
                    position: Date.now(),
                  });
              }}
            >
              <option value="">+ Playlist</option>
              {editable.map((list) => (
                <option key={list.id} value={list.id}>
                  {list.name}
                </option>
              ))}
            </select>
          )}
    </div>
  );
}
