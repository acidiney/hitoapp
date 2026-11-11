import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";
import { Play } from "lucide-react";
import { time } from "../../../shared/format";
import type { Mutate } from "../../../shared/hooks/useMutation";
import Artwork from "../../artwork/components/Artwork";
import { usePlayer } from "../../playback/components/PlaybackProvider";
import TrackNames, {
  albumHref,
  artistHref,
} from "../../playback/components/TrackNames";
import TrackRowActions from "../../playback/components/TrackRowActions";
import { trackHref } from "../../playback/components/TrackRowMenu";
import StatusDot from "../../../shared/ui/StatusDot";
import {
  rowPlayState,
  useNowPlaying,
} from "../../playback/hooks/useNowPlaying";
import {
  PlayingBars,
  PlayingStatus,
} from "../../playback/components/PlayingBars";
import { usePlayTracks } from "../../playback/hooks/usePlayTracks";
import type { TrackRow } from "../../playback/components/TrackTable";
import { findAudioHref } from "../../recordings/model/trackTabs";
import { RESULT_ROW } from "../hooks/useResultKeys";

function Availability({ track }: { track: TrackRow }) {
  if (track.available) return <StatusDot status="ready" detail="Local" />;
  if (track.plays_from_services === true)
    return <StatusDot status="ready" detail="from your services" />;
  if (track.plays_from_services === false)
    return (
      <>
        <StatusDot status="none" />
        <Link className="result-row-action" href={findAudioHref(track.id)}>
          Find
        </Link>
      </>
    );
  return null;
}

export default function TrackResultRows({
  tracks,
  playlists,
  mutate,
}: {
  tracks: TrackRow[];
  playlists: Schema<"Playlist">[];
  mutate: Mutate;
}) {
  const player = usePlayer();
  const playTracks = usePlayTracks(tracks);
  const nowPlaying = useNowPlaying();
  return (
    <ul className="result-rows" aria-label="Tracks">
      {tracks.map((track, index) => {
        const playState = rowPlayState(nowPlaying, track);
        return (
          <li
            key={track.row_id}
            aria-current={playState ? "true" : undefined}
            className={playState ? "result-row playing-row" : "result-row"}
            tabIndex={-1}
            {...{ [RESULT_ROW]: "" }}
            onKeyDown={(event) => {
              if (event.target !== event.currentTarget || event.key !== "Enter")
                return;
              event.preventDefault();
              if (event.metaKey || event.ctrlKey)
                void player.addToQueue?.(track);
              else void playTracks.playFrom(index, track.playback_start);
            }}
          >
            <button
              type="button"
              className="result-row-cover"
              aria-label={`Play ${track.title}`}
              onClick={() =>
                void playTracks.playFrom(index, track.playback_start)
              }
            >
              <Artwork
                release={track.release_id ?? undefined}
                recording={track.id}
                fallback={{
                  id: track.release_id ?? track.id,
                  title: track.album ?? track.title,
                  artist: track.artist,
                }}
              />
              {playState && <PlayingBars state={playState} />}
              <Play size={16} fill="currentColor" aria-hidden="true" />
            </button>
            {playState && <PlayingStatus state={playState} />}
            <span className="result-row-copy">
              <Link className="result-row-title" href={trackHref(track.id)}>
                {track.title}
              </Link>
              <TrackNames
                className="result-row-sub"
                artist={track.artist}
                artistHref={artistHref(track)}
                credited={track.credited_artists}
                album={track.album ?? null}
                albumHref={albumHref(track)}
              />
            </span>
            <span className="result-row-state">
              <Availability track={track} />
            </span>
            <span className="result-row-time">
              <span>{time(track.duration_ms / 1000)}</span>
            </span>
            <TrackRowActions
              track={track}
              playlists={playlists}
              mutate={mutate}
            />
          </li>
        );
      })}
    </ul>
  );
}
