import type { Schema } from "../../../shared/schema";
import { time } from "../../../shared/format";

import { usePlayer } from "../../playback/components/PlaybackProvider";
import { foundKey, useCatalogTrackPlay } from "../hooks/useCatalogTrackPlay";
import Button from "../../../shared/ui/Button";
import EditionTrackRow from "./EditionTrackRow";
import TrackRowActions from "../../playback/components/TrackRowActions";
import { useAlbumMerge } from "../../catalog/components/AlbumMergeContext";
import { creditText } from "../model/catalogAlbumRows";

type Comparison = Extract<
  Schema<"AlbumEditionTrackListAvailability">,
  { status: "available" }
>;
type Occurrence = Comparison["edition"]["media"][number]["tracks"][number];

const kinds = { audio: "Audio", pregap: "Pregap", data: "Data" };

export type AudioSource = { release: string; recording: string };

export default function AlbumEditionTrackList({
  account,
  source,
  comparison,
  pending,
  error,
  retry,
}: {
  account: string;
  source: AudioSource;
  comparison: Comparison | null;
  pending: boolean;
  error: string;
  retry: () => Promise<void>;
}) {
  const player = usePlayer();
  const merge = useAlbumMerge(comparison !== null);
  const catalog = useCatalogTrackPlay(
    account,
    comparison?.adoption ?? null,
    player,
    `${source.release}/${comparison?.edition.id ?? ""}`,
  );
  const linked = new Map<string, Schema<"Recording">>(
    comparison?.matches.map((match) => [
      match.occurrence_id,
      match.recording,
    ]) ?? [],
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
    const library =
      linked.get(track.id) ??
      (track.recording_id
        ? catalog.saved.get(track.recording_id)
        : undefined) ??
      catalog.saved.get(foundKey(comparison?.edition.id ?? "", track.id));
    const failure = catalog.failed?.id === track.id ? catalog.failed : null;
    const working = catalog.pending === track.id;
    const credit = creditText(track.artist_credit);
    const common = {
      number: track.number ?? <span title="Track number not provided">–</span>,
      title: track.title,
      credit,
      time: track.duration_ms == null ? "" : time(track.duration_ms / 1000),
      failure: failure?.message,
    };
    if (library)
      return (
        <EditionTrackRow
          key={track.id}
          {...common}
          state="library"
          actions={libraryActions(library)}
          detail={library.title === track.title ? undefined : library.title}
          play={{
            label: `Play ${library.title}`,
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
            disabled: catalog.pending !== null,
            onPlay: () => void catalog.play(track.id, track.recording_id!),
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
                disabled: catalog.pending !== null,
                onPlay: () => void catalog.findAndPlay(track.id, search(track)),
              }
            : undefined
        }
      />
    );
  }

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
          {comparison.is_album_release && (
            <p className="edition-note">
              This is the release this album was saved from.
            </p>
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
          {comparison.library_only.length > 0 && (
            <section aria-label="Not in this edition">
              <h3 className="edition-group-title">Not on this edition</h3>
              <ul
                aria-label="In this album only"
                className="edition-track-rows"
              >
                {comparison.library_only.map((track) => (
                  <EditionTrackRow
                    key={track.entry_id}
                    number="–"
                    title={track.title}
                    state="library"
                    actions={libraryActions(track)}
                    credit={track.artist}
                    time={
                      track.duration_ms ? time(track.duration_ms / 1000) : ""
                    }
                    play={{
                      label: `Play ${track.title}`,
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
