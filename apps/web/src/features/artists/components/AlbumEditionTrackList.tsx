import type { Schema } from "../../../shared/schema";
import { useMemo, useState } from "react";
import { time } from "../../../shared/format";

import { usePlayer } from "../../playback/components/PlaybackProvider";
import { foundKey, useCatalogTrackPlay } from "../hooks/useCatalogTrackPlay";
import Button from "../../../shared/ui/Button";
import EditionTrackRow, { type EditionRowState } from "./EditionTrackRow";
import EditionToolbar, { type EditionFilter } from "./EditionToolbar";
import TrackRowActions from "../../playback/components/TrackRowActions";
import {
  useAlbumMerge,
  useAlbumProgress,
  useEditionPlayPublish,
} from "../../catalog/components/AlbumMergeContext";
import { albumEditionApi } from "../api/albumEditionApi";
import { useEditionPlayback } from "../hooks/useCatalogPlayback";
import { creditText } from "../model/catalogAlbumRows";

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
  const merge = useAlbumMerge(comparison !== null);
  const [filter, setFilter] = useState<EditionFilter>("all");
  const catalog = useCatalogTrackPlay(
    account,
    comparison?.adoption ?? null,
    player,
    `${source.release}/${comparison?.edition.id ?? ""}`,
  );
  const play = (recording: Schema<"Recording">) =>
    void player.playQueue([recording], 0);

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
  useEditionPlayPublish(
    comparison === null
      ? null
      : {
          tracks: audio.length,
          durationMs: audio.reduce(
            (sum, track) => sum + (track.duration_ms ?? 0),
            0,
          ),
          consentOff: playback.consentOff,
          halted: playback.halted,
        },
    { play: () => playback.start(0), shuffle: playback.shuffle },
  );

  function libraryFor(track: Occurrence) {
    return owners.get(track.id) ?? playback.recordings.get(track.id);
  }

  const owned = audio.filter((track) => libraryFor(track)).length;
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

  function ownedState(recording: Schema<"Recording">): {
    state: EditionRowState;
    detail?: string;
  } {
    if (recording.available) return { state: "ready", detail: "Local" };
    if (recording.plays_from_services)
      return { state: "ready", detail: "from your services" };
    return { state: "library" };
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
          {...ownedState(library)}
          actions={libraryActions(library)}
          play={{
            label: `Play ${library.title}`,
            menuLabel: "Play",
            disabled: false,
            onPlay: () => play(library),
          }}
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
    const state = working ? "working" : "absent";
    const adoptable = comparison?.adoption && track.recording_id != null;
    const detail = working
      ? adoptable
        ? "Saving…"
        : "Finding audio…"
      : missing;
    if (adoptable)
      return (
        <EditionTrackRow
          key={track.id}
          {...common}
          state={state}
          detail={detail}
          play={{
            label: `Play ${track.title}`,
            menuLabel: "Play",
            disabled: catalog.pending !== null,
            onPlay: () => void catalog.play(track.id, track.recording_id!),
          }}
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
        play={
          comparison
            ? {
                label: `Find audio for ${track.title}`,
                menuLabel: "Find audio and play",
                disabled: catalog.pending !== null,
                onPlay: () => void catalog.findAndPlay(track.id, search(track)),
              }
            : undefined
        }
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
      {pending && <p role="status">Loading edition track list…</p>}
      {error && (
        <>
          <p role="alert">{error}</p>
          <Button type="button" disabled={pending} onClick={() => void retry()}>
            Try track list again
          </Button>
        </>
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
            />
          )}
          {comparison.edition.media.map((medium) => (
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
              </ol>
            </section>
          ))}
          {filter === "all" && comparison.library_only.length > 0 && (
            <section aria-label="Not in this edition">
              <h3 className="edition-group-title">
                In your library, not in this edition
              </h3>
              <ul
                aria-label="In this album only"
                className="edition-track-rows"
              >
                {comparison.library_only.map((track) => (
                  <EditionTrackRow
                    key={track.entry_id}
                    number="–"
                    title={track.title}
                    {...ownedState(track)}
                    actions={libraryActions(track)}
                    credit={track.artist}
                    time={
                      track.duration_ms ? time(track.duration_ms / 1000) : ""
                    }
                    play={{
                      label: `Play ${track.title}`,
                      menuLabel: "Play",
                      disabled: false,
                      onPlay: () => play(track),
                    }}
                  />
                ))}
              </ul>
            </section>
          )}
        </>
      )}
    </section>
  );
}
