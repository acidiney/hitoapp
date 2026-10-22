import Artwork from "../Artwork";
import TrackActions from "../../shared/ui/TrackActions";
import type { TrackRow } from "../../shared/ui/TrackTable";
import { time } from "../../shared/format";
import type { Mutate } from "../../shared/hooks/useMutation";
import { log } from "../../shared/log";
import { usePlayer } from "../../player/PlaybackProvider";
import type { PlaybackStart } from "../../player/model";
import type { Playlist, PodcastEpisode, RecordingProgress } from "../../shared/types";

type EpisodeTrack = TrackRow & { playback_start: PlaybackStart };

export default function PodcastEpisodes({
  episodes,
  progress,
  playlists,
  mutate,
  onMutationAccepted,
}: {
  episodes: PodcastEpisode[];
  progress: RecordingProgress[];
  playlists: Playlist[];
  mutate: Mutate;
  onMutationAccepted: () => void;
}) {
  const player = usePlayer();
  const saved = new Map(progress.map((item) => [item.recording_id, item]));
  const tracks = episodes.map<EpisodeTrack>((episode) => ({
    ...episode.recording,
    row_id: episode.episode_id,
    playback_start: {
      kind: "saved",
      position: (saved.get(episode.recording.id)?.position_ms ?? 0) / 1000,
    },
  }));
  function play(index: number, start: PlaybackStart) {
    log.info("podcasts.episode_play_requested", {
      recording_id: tracks[index].id,
      mode: start.kind,
      position_ms: Math.round(start.position * 1000),
      count: tracks.length,
    });
    void player.playQueue(tracks, index, start);
  }
  return (
    <div className="table-scroll">
      <table className="track-table" aria-label="Episodes">
        <thead>
          <tr>
            <th>Episode</th>
            <th>Title</th>
            <th className="artist-column">Host</th>
            <th className="duration-column">Time</th>
            <th>
              <span className="sr-only">Actions</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {tracks.map((track, index) => {
            const start = track.playback_start;
            return (
              <tr key={track.row_id}>
                <td>
                  <button
                    className="button"
                    aria-label={`${start.position > 0 ? "Resume" : "Play"} ${track.title}`}
                    onClick={() => play(index, start)}
                  >
                    {start.position > 0
                      ? `Resume ${time(start.position)}`
                      : "Play"}
                  </button>
                </td>
                <td className="track-cell">
                  <Artwork
                    recording={track.id}
                    release={track.release_id ?? undefined}
                  />
                  <div>
                    <span>{track.title}</span>
                    {!track.available && (
                      <small>Unavailable on this device</small>
                    )}
                    <span className="mobile-artist">{track.artist}</span>
                  </div>
                </td>
                <td className="artist-column muted">{track.artist}</td>
                <td className="duration-column muted">
                  {time(track.duration_ms / 1000)}
                </td>
                <td>
                  <button
                    className="button"
                    aria-label={`Start over ${track.title}`}
                    onClick={() =>
                      play(index, { kind: "position", position: 0 })
                    }
                  >
                    Start over
                  </button>
                  <TrackActions
                    track={track}
                    playlists={playlists}
                    mutate={mutate}
                    onMutationAccepted={onMutationAccepted}
                  />
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
