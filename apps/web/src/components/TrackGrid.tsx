import { Play } from "lucide-react";
import type { Mutate } from "../hooks/useMutation";
import type { Playlist } from "../types";
import type { TrackRow } from "./TrackTable";
import { usePlayer } from "../player/PlaybackProvider";
import Artwork from "./Artwork";
import TrackActions from "./TrackActions";

export default function TrackGrid({
  tracks,
  playlists,
  mutate,
}: {
  tracks: TrackRow[];
  playlists: Playlist[];
  mutate: Mutate;
}) {
  const player = usePlayer();
  return (
    <div className="track-grid">
      {tracks.map((track, index) => (
        <article className="track-card" key={track.row_id}>
          <div className="track-card-cover">
            <Artwork recording={track.id} />
            <button
              className="card-play"
              aria-label={`Play ${track.title}`}
              disabled={!track.available}
              onClick={() => void player.playQueue(tracks, index)}
            >
              <Play size={22} fill="currentColor" />
            </button>
          </div>
          <div className="track-card-copy">
            <h3>{track.title}</h3>
            <p>{track.artist}</p>
            {!track.available && <small>Unavailable on this device</small>}
          </div>
          <TrackActions track={track} playlists={playlists} mutate={mutate} />
        </article>
      ))}
    </div>
  );
}
