import { router } from "@inertiajs/react";
import type { Schema } from "../../../shared/schema";
import type { Mutate } from "../../../shared/hooks/useMutation";
import { time } from "../../../shared/format";
import TrackRowActions from "../../playback/components/TrackRowActions";
import {
  rowPlayState,
  useNowPlaying,
} from "../../playback/hooks/useNowPlaying";
import type {
  CatalogTrack,
  CatalogTrackControls,
} from "../hooks/useCatalogTracks";
import EditionTrackRow, { ownedRowState } from "./EditionTrackRow";
import type { EditionFilter } from "./EditionToolbar";

const misses = {
  not_found: "No audio found",
  failed: "Couldn't find audio",
};

export default function CatalogTrackList({
  tracks,
  controls,
  filter,
  playlists,
  mutate,
}: {
  tracks: CatalogTrack[];
  controls: CatalogTrackControls;
  filter: EditionFilter;
  playlists: Schema<"Playlist">[];
  mutate: Mutate;
}) {
  const now = useNowPlaying();
  function row(track: CatalogTrack, index: number) {
    const { recording } = track;
    if (filter === "library" && !recording) return null;
    const common = {
      number: track.number,
      title: track.title,
      credit: track.credit,
      time: track.durationMs == null ? "" : time(track.durationMs / 1000),
      play: {
        label: `Play ${track.title}`,
        menuLabel: "Play",
        disabled: false,
        onPlay: () => controls.play(index),
      },
    };
    if (recording)
      return (
        <EditionTrackRow
          key={track.id}
          {...common}
          {...ownedRowState(recording)}
          playing={rowPlayState(now, recording)}
          actions={
            <TrackRowActions
              track={{ ...recording, row_id: recording.id }}
              playlists={playlists}
              mutate={mutate}
              liking={false}
            />
          }
        />
      );
    const finding = track.status === "finding";
    const missed = track.status === "not_found" || track.status === "failed";
    return (
      <EditionTrackRow
        key={track.id}
        {...common}
        state={finding ? "working" : "absent"}
        detail={
          finding
            ? "Finding audio…"
            : missed
              ? misses[track.status as keyof typeof misses]
              : undefined
        }
        add={{
          label: `Add ${track.title} to your library`,
          disabled: finding,
          onAdd: () => controls.find(index),
        }}
        more={[
          {
            label: "Track details",
            disabled: finding,
            onSelect: () => controls.open(index),
          },
          {
            label: "Find audio and like",
            disabled: finding,
            onSelect: () => controls.like(index),
          },
          ...(missed
            ? [
                {
                  label: "Connect services",
                  onSelect: () => router.visit("/settings?section=music"),
                },
              ]
            : []),
        ]}
      />
    );
  }

  return (
    <section className="edition-tracks">
      <ol aria-label="Tracks" className="edition-track-rows">
        {tracks.map(row)}
      </ol>
    </section>
  );
}
