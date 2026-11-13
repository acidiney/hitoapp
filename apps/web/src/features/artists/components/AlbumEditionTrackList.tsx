import type { Schema } from "../../../shared/schema";
import { useMemo, useState } from "react";
import { time } from "../../../shared/format";

import { usePlayer } from "../../playback/components/PlaybackProvider";
import { foundKey, useCatalogTrackPlay } from "../hooks/useCatalogTrackPlay";
import MatchPanel from "../../catalog/components/MatchPanel";
import EditionTrackRow, { ownedRowState } from "./EditionTrackRow";
import EditionToolbar, { type EditionFilter } from "./EditionToolbar";
import TrackRowActions from "../../playback/components/TrackRowActions";
import {
  useAlbumMerge,
  useAlbumProgress,
  useEditionLoading,
  useEditionPlayPublish,
} from "../../catalog/components/AlbumMergeContext";
import { albumEditionApi } from "../api/albumEditionApi";
import { useEditionPlayback } from "../hooks/useCatalogPlayback";
import { creditText } from "../model/catalogAlbumRows";
import {
  rowPlayState,
  useNowPlaying,
} from "../../playback/hooks/useNowPlaying";

type Comparison = Extract<
  Schema<"AlbumEditionTrackListAvailability">,
  { status: "available" }
>;
type Occurrence = Comparison["edition"]["media"][number]["tracks"][number];

const kinds = { audio: "Audio", pregap: "Pregap", data: "Data" };

export type EditionControls = {
  change?: () => void;
  forget?: () => void;
  busy: boolean;
};

export type AudioSource = { release: string; recording: string };

export default function AlbumEditionTrackList({
  account,
  source,
  comparison,
  pending,
  error,
  retry,
  edition,
}: {
  account: string;
  source: AudioSource;
  comparison: Comparison | null;
  pending: boolean;
  error: string;
  retry: () => Promise<void>;
  edition?: EditionControls;
}) {
  const player = usePlayer();
  const now = useNowPlaying();
  const merge = useAlbumMerge(comparison !== null);
  const [chosenFilter, setFilter] = useState<EditionFilter>("all");
  const catalog = useCatalogTrackPlay(
    account,
    comparison?.adoption ?? null,
    player,
    `${source.release}/${comparison?.edition.id ?? ""}`,
  );

  const search = (track: Occurrence) => ({
    release: source.release,
    request: {
      recording_id: source.recording,
      association_id: comparison!.association_id,
      release_group_id: comparison!.edition.release_group_id,
      edition_id: comparison!.edition.id,
      occurrence_id: track.id,
    },
  });

  const audio = useMemo(
    () =>
      comparison?.edition.media.flatMap((medium) =>
        medium.tracks.filter((track) => track.kind === "audio"),
      ) ?? [],
    [comparison],
  );
  const owners = useMemo(() => {
    const owned = new Map<string, Schema<"Recording">>();
    for (const match of comparison?.matches ?? [])
      owned.set(match.occurrence_id, match.recording);
    for (const track of audio) {
      const saved =
        (track.recording_id
          ? catalog.saved.get(track.recording_id)
          : undefined) ??
        catalog.saved.get(foundKey(comparison?.edition.id ?? "", track.id));
      if (saved && !owned.has(track.id)) owned.set(track.id, saved);
    }
    return owned;
  }, [comparison, audio, catalog.saved]);
  const queue = useMemo(
    () =>
      audio
        .filter(
          (track) =>
            owners.has(track.id) ||
            !comparison?.attached_occurrences.includes(track.id),
        )
        .map((track) => track.id),
    [audio, owners, comparison],
  );
  const playback = useEditionPlayback(
    {
      scope: JSON.stringify([account, source.release, source.recording]),
      find: (edition, occurrence, signal) =>
        albumEditionApi.audio(
          account,
          source.release,
          {
            recording_id: source.recording,
            association_id: comparison!.association_id,
            release_group_id: comparison!.edition.release_group_id,
            edition_id: edition,
            occurrence_id: occurrence,
          },
          signal,
        ),
    },
    comparison?.edition.id ?? null,
    queue,
    owners,
  );
  const waiting = comparison === null && !error;
  useEditionLoading(waiting);
  useEditionPlayPublish(
    comparison === null
      ? null
      : {
          tracks: comparison.stats.audio_tracks,
          durationMs: comparison.stats.total_duration_ms,
          consentOff: playback.consentOff,
          halted: playback.halted,
        },
    { play: () => startFrom(0), shuffle: playback.shuffle },
  );

  function startFrom(at: number) {
    playback.seed(owners);
    return playback.start(at);
  }

  function playFrom(track: Occurrence, title: string) {
    const at = queue.indexOf(track.id);
    if (at < 0) return undefined;
    return {
      label: `Play ${title}`,
      menuLabel: "Play",
      disabled: false,
      onPlay: () => void startFrom(at),
    };
  }

  function searchState(track: Occurrence) {
    const status = playback.rows.get(track.id)?.status;
    if (status === "finding")
      return { state: "working" as const, detail: "Finding audio…" };
    if (status === "not_found")
      return { state: "absent" as const, detail: "No audio found" };
    if (status === "failed")
      return { state: "absent" as const, detail: "Couldn't find audio" };
    return null;
  }

  function libraryFor(track: Occurrence) {
    return owners.get(track.id) ?? playback.recordings.get(track.id);
  }

  const owned = audio.filter((track) => libraryFor(track)).length;
  const filter = owned > 0 ? chosenFilter : "all";
  const absent = comparison?.library_only.length ?? 0;
  useAlbumProgress(
    comparison === null
      ? null
      : { owned, total: audio.length, elsewhere: absent },
  );

  function libraryActions(recording: Schema<"Recording">) {
    if (!merge.mutate) return undefined;
    return (
      <TrackRowActions
        track={{
          ...recording,
          row_id: recording.id,
          liked: (recording as { liked?: boolean }).liked ?? false,
        }}
        playlists={merge.playlists}
        mutate={merge.mutate}
        liking={false}
      />
    );
  }

  function row(track: Occurrence) {
    const library = libraryFor(track);
    if (filter === "library" && !library) return null;
    const failure = catalog.failed?.id === track.id ? catalog.failed : null;
    const working = catalog.pending === track.id;
    const common = {
      number: track.number ?? <span title="Track number not provided">–</span>,
      title: track.title,
      credit: creditText(track.artist_credit),
      time: track.duration_ms == null ? "" : time(track.duration_ms / 1000),
      failure: failure?.message,
    };
    if (library)
      return (
        <EditionTrackRow
          key={track.id}
          {...common}
          {...ownedRowState(library)}
          playing={rowPlayState(now, library)}
          actions={libraryActions(library)}
          play={playFrom(track, library.title)}
        />
      );
    if (track.kind !== "audio")
      return (
        <EditionTrackRow
          key={track.id}
          {...common}
          state="unplayable"
          detail={kinds[track.kind]}
        />
      );
    if (comparison?.attached_occurrences.includes(track.id))
      return (
        <EditionTrackRow
          key={track.id}
          {...common}
          state="attached"
          detail="Already attached as audio to another recording"
        />
      );
    const missing =
      track.recording_id == null
        ? "No provider identity for this track"
        : undefined;
    const adoptable = comparison?.adoption && track.recording_id != null;
    const searched = searchState(track);
    const state = working ? "working" : (searched?.state ?? "absent");
    const detail = working
      ? adoptable
        ? "Saving…"
        : "Finding audio…"
      : (searched?.detail ?? missing);
    if (adoptable)
      return (
        <EditionTrackRow
          key={track.id}
          {...common}
          state={state}
          detail={detail}
          play={playFrom(track, track.title)}
          add={{
            label: `Add ${track.title} to your library`,
            disabled: catalog.pending !== null,
            onAdd: () => void catalog.save(track.id, track.recording_id!),
          }}
        />
      );
    return (
      <EditionTrackRow
        key={track.id}
        {...common}
        state={state}
        detail={detail}
        play={playFrom(track, track.title)}
        add={
          comparison
            ? {
                label: `Add ${track.title} to your library`,
                disabled: catalog.pending !== null,
                onAdd: () => void catalog.findAndSave(track.id, search(track)),
              }
            : undefined
        }
      />
    );
  }

  function libraryOnlyRow(
    track: Comparison["library_only"][number],
    index: number,
    tracks: Comparison["library_only"],
  ) {
    return (
      <EditionTrackRow
        key={track.entry_id}
        number="–"
        title={track.title}
        {...ownedRowState(track)}
        playing={rowPlayState(now, track)}
        actions={libraryActions(track)}
        credit={track.artist}
        time={track.duration_ms ? time(track.duration_ms / 1000) : ""}
        play={{
          label: `Play ${track.title}`,
          menuLabel: "Play",
          disabled: false,
          onPlay: () => void player.playQueue(tracks, index),
        }}
      />
    );
  }

  const menu = [
    ...(edition?.change
      ? [
          {
            label: "Change edition",
            disabled: edition.busy,
            onSelect: edition.change,
          },
        ]
      : []),
    ...(edition?.forget
      ? [
          {
            label: "Forget edition",
            disabled: edition.busy,
            onSelect: edition.forget,
          },
        ]
      : []),
  ];

  return (
    <section
      aria-label="Album edition track list"
      aria-busy={pending}
      className="edition-tracks"
    >
      {waiting && (
        <div className="track-list-loading">
          <span role="status" className="state-card-title">
            Loading track list…
          </span>
          <span className="release-skeleton-line" aria-hidden="true" />
          <span className="release-skeleton-line" aria-hidden="true" />
        </div>
      )}
      {error && (
        <MatchPanel title="Showing only your tracks">
          <p role="alert" className="match-alert">
            {error} Your tracks are listed and play as usual. The full track
            list and adding the rest need the edition.
          </p>
          <div className="match-actions">
            <button
              type="button"
              className="text-action"
              disabled={pending}
              onClick={() => void retry()}
            >
              Try again
            </button>
          </div>
        </MatchPanel>
      )}
      {comparison && (
        <>
          {edition && (
            <EditionToolbar
              name={comparison.edition.title}
              total={audio.length}
              owned={owned}
              filter={filter}
              onFilter={setFilter}
              items={menu}
              source="your catalog service"
            />
          )}
          {comparison.edition.media.map((medium, index) => (
            <section
              key={medium.position}
              aria-label={`Disc ${medium.position}`}
            >
              <div
                className={
                  comparison.edition.media.length > 1
                    ? "disc-header"
                    : "sr-only"
                }
              >
                <h6>Disc {medium.position}</h6>
                {medium.title && <p>{medium.title}</p>}
              </div>
              <ol
                aria-label={`Disc ${medium.position} tracks`}
                className="edition-track-rows"
              >
                {medium.tracks.map(row)}
                {index === comparison.edition.media.length - 1 &&
                  comparison.library_only.map(libraryOnlyRow)}
              </ol>
            </section>
          ))}
          {comparison.edition.media.length === 0 &&
            comparison.library_only.length > 0 && (
              <ol aria-label="Tracks" className="edition-track-rows">
                {comparison.library_only.map(libraryOnlyRow)}
              </ol>
            )}
        </>
      )}
    </section>
  );
}
