import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";
import { Play, RotateCcw } from "lucide-react";
import TrackRowActions from "../../playback/components/TrackRowActions";
import ProgressBar from "../../../shared/ui/ProgressBar";
import { longDuration, time } from "../../../shared/format";
import type { Mutate } from "../../../shared/hooks/useMutation";
import type { PlaybackStart } from "../../playback/model";

import type { EpisodeRow } from "../hooks/useEpisodeRows";

export default function PodcastEpisodes({
  rows,
  playlists,
  mutate,
  play,
  onMutationAccepted,
}: {
  rows: EpisodeRow[];
  playlists: Schema<"Playlist">[];
  mutate: Mutate;
  play: (index: number, start: PlaybackStart) => void;
  onMutationAccepted: () => void;
}) {
  return (
    <ol className="episode-list" aria-label="Episodes">
      {rows.map(({ index, track, state, positionMs }) => {
        const resuming = positionMs > 0 && state.kind === "remaining";
        const start: PlaybackStart =
          state.kind === "played"
            ? { kind: "position", position: 0 }
            : track.playback_start;
        return (
          <li
            key={track.row_id}
            className="episode-row"
            data-state={state.kind}
          >
            <span className="episode-length">
              {track.duration_ms > 0
                ? longDuration(track.duration_ms / 1000)
                : "—"}
            </span>
            <div className="episode-copy">
              <Link
                className="episode-title"
                href={`/recordings/${encodeURIComponent(track.id)}`}
              >
                {track.title}
              </Link>
              <span className="episode-sub">
                {!track.available
                  ? "Unavailable on this device"
                  : track.artist || "Episode"}
              </span>
            </div>
            <div className="episode-state">
              <span className="episode-state-label">{state.label}</span>
              {state.kind !== "new" && (
                <ProgressBar
                  percent={state.percent}
                  label={`Progress of ${track.title}`}
                />
              )}
            </div>
            <div className="episode-actions">
              <button
                type="button"
                className="round-button solid"
                aria-label={`${resuming ? "Resume" : "Play"} ${track.title}`}
                title={resuming ? `Resume at ${time(start.position)}` : "Play"}
                onClick={() => play(index, start)}
              >
                <Play size={15} fill="currentColor" />
              </button>
              {resuming && (
                <button
                  type="button"
                  className="round-button"
                  aria-label={`Start over ${track.title}`}
                  title="Start over"
                  onClick={() => play(index, { kind: "position", position: 0 })}
                >
                  <RotateCcw size={15} />
                </button>
              )}
              <TrackRowActions
                track={track}
                playlists={playlists}
                mutate={mutate}
                onMutationAccepted={onMutationAccepted}
              />
            </div>
          </li>
        );
      })}
    </ol>
  );
}
