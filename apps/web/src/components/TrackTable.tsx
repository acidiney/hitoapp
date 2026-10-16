import { useSyncExternalStore } from "react";
import { Link } from "@inertiajs/react";
import { Play } from "lucide-react";
import { time } from "../format";
import { usePlayer } from "../player/PlaybackProvider";
import type { Track } from "../playback";
import type { PlaybackStart } from "../player/model";
import type { Playlist } from "../types";
import type { Mutate } from "../hooks/useMutation";
import TrackActions from "./TrackActions";
import Artwork from "./Artwork";

export type TrackRow = Track & {
  row_id: string;
  liked: boolean;
  album?: string | null;
  release_id?: string | null;
  disc_number?: number | null;
  track_number?: number | null;
  playback_start?: PlaybackStart;
  entry_id?: string;
};

export default function TrackTable({
  tracks,
  offset = 0,
  playlists,
  playlist,
  mutate,
}: {
  tracks: TrackRow[];
  offset?: number;
  playlists: Playlist[];
  playlist?: Playlist;
  mutate: Mutate;
}) {
  const player = usePlayer();
  const currentId = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().current?.id,
  );
  function play(index: number) {
    void player.playQueue(tracks, index, tracks[index].playback_start);
  }
  return (
    <div className="table-scroll">
      <table className="track-table">
        <thead>
          <tr>
            <th className="number-column">
              <span className="sr-only">Play</span>#
            </th>
            <th>Title</th>
            <th className="artist-column">Artist</th>
            <th className="album-column">Album</th>
            <th className="duration-column">Time</th>
            <th>
              <span className="sr-only">Actions</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {tracks.map((track, index) => (
            <tr
              key={track.row_id}
              className={currentId === track.id ? "playing-row" : ""}
            >
              <td>
                <button
                  className="row-play icon-button"
                  aria-label={`Play ${track.title}`}
                  disabled={!track.available}
                  onClick={() => play(index)}
                >
                  <span>
                    {track.track_number != null
                      ? `${track.disc_number && track.disc_number > 1 ? `${track.disc_number}.` : ""}${track.track_number}`
                      : index + 1 + offset}
                  </span>
                  <Play size={15} />
                </button>
              </td>
              <td className="track-cell">
                <Artwork
                  release={track.release_id ?? undefined}
                  recording={track.id}
                />
                <div>
                  <button
                    className="track-title"
                    disabled={!track.available}
                    onClick={() => play(index)}
                  >
                    {track.title}
                  </button>
                  {!track.available && (
                    <small>Unavailable on this device</small>
                  )}
                  <span className="mobile-artist">{track.artist}</span>
                </div>
              </td>
              <td className="artist-column muted">{track.artist}</td>
              <td className="album-column muted">
                {track.release_id ? (
                  <Link
                    href={`/albums/${encodeURIComponent(track.release_id)}`}
                  >
                    {track.album ?? "Unknown album"}
                  </Link>
                ) : (
                  (track.album ?? "—")
                )}
              </td>
              <td className="duration-column muted">
                {time(track.duration_ms / 1000)}
              </td>
              <td>
                <TrackActions
                  track={track}
                  playlists={playlists}
                  playlist={playlist}
                  mutate={mutate}
                />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
