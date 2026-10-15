import { router } from "@inertiajs/react";
import { Heart, X } from "lucide-react";
import type { Action, Playlist } from "../types";
import type { Mutate } from "../hooks/useMutation";
import type { TrackRow } from "./TrackTable";

export default function TrackActions({
  track,
  playlists,
  playlist,
  mutate,
}: {
  track: TrackRow;
  playlists: Playlist[];
  playlist?: Playlist;
  mutate: Mutate;
}) {
  const editable = playlists.filter((list) => list.kind === "local");
  const entryId = track.entry_id;
  async function update(action: Action) {
    if ((await mutate(action)).ok) router.reload();
  }
  return (
    <div className="track-actions">
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
      {playlist
        ? playlist.kind === "local" &&
          entryId && (
            <button
              className="icon-button"
              aria-label={`Remove ${track.title} from playlist`}
              onClick={() =>
                void update({
                  kind: "entry_remove",
                  playlist_id: playlist.id,
                  entry_id: entryId,
                })
              }
            >
              <X size={16} />
            </button>
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
                    entry_id: crypto.randomUUID(),
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
