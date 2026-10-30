import type { Schema } from "../../../shared/schema";
import { log } from "../../../shared/log";
import { usePlayer } from "../../playback/components/PlaybackProvider";
import type { TrackRow } from "../../playback/components/TrackTable";
import type { PlaybackStart } from "../../playback/model";

import { episodeState, type EpisodeState } from "../model/episodeState";

export type EpisodeTrack = TrackRow & { playback_start: PlaybackStart };
export type EpisodeRow = {
  index: number;
  track: EpisodeTrack;
  state: EpisodeState;
  positionMs: number;
};

export function useEpisodeRows(
  episodes: Schema<"PodcastEpisode">[],
  progress: Schema<"RecordingProgress">[],
) {
  const player = usePlayer();
  const saved = new Map(progress.map((item) => [item.recording_id, item]));
  const rows = episodes.map<EpisodeRow>((episode, index) => {
    const positionMs = saved.get(episode.recording.id)?.position_ms ?? 0;
    return {
      index,
      positionMs,
      state: episodeState(episode.recording.duration_ms, positionMs),
      track: {
        ...episode.recording,
        row_id: episode.episode_id,
        playback_start: { kind: "saved", position: positionMs / 1000 },
      },
    };
  });
  const tracks = rows.map((row) => row.track);
  function play(index: number, start: PlaybackStart) {
    log.info("podcasts.episode_play_requested", {
      recording_id: tracks[index].id,
      mode: start.kind,
      position_ms: Math.round(start.position * 1000),
      count: tracks.length,
    });
    void player.playQueue(tracks, index, start);
  }
  const resumable = rows.find(
    (row) => row.state.kind === "remaining" && row.track.available,
  );
  const latest = rows.find((row) => row.track.available);
  return { rows, play, resumable, latest };
}
