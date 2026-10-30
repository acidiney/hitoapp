import type { Schema } from "../../../shared/schema";
import { time } from "../../../shared/format";

import { usePlayer } from "../../playback/components/PlaybackProvider";
import { foundKey, useCatalogTrackPlay } from "../hooks/useCatalogTrackPlay";
import Button from "../../../shared/ui/Button";

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

  function row(track: Occurrence) {
    const library =
      linked.get(track.id) ??
      (track.recording_id
        ? catalog.saved.get(track.recording_id)
        : undefined) ??
      catalog.saved.get(foundKey(comparison?.edition.id ?? "", track.id));
    const failure = catalog.failed?.id === track.id ? catalog.failed : null;
    return (
      <li key={track.id}>
        <p>{track.number ?? "Track number not provided"}</p>
        <p>{track.title}</p>
        {track.kind !== "audio" && <p>{kinds[track.kind]}</p>}
        <p>
          {track.duration_ms == null
            ? "Duration not provided"
            : time(track.duration_ms / 1000)}
        </p>
        {library ? (
          <>
            <p>In your library as {library.title}</p>
            <Button
              type="button"
              aria-label={`Play ${library.title}`}
              onClick={() => play(library)}
            >
              Play
            </Button>
          </>
        ) : track.kind !== "audio" ? (
          <p>Not playable audio</p>
        ) : (
          <>
            <p>Not in your library</p>
            {track.recording_id == null && (
              <p>No provider identity for this track</p>
            )}
            {comparison?.attached_occurrences.includes(track.id) ? (
              <p>Already attached as audio to another recording</p>
            ) : comparison?.adoption && track.recording_id != null ? (
              <Button
                type="button"
                disabled={catalog.pending !== null}
                aria-label={`Play ${track.title}`}
                onClick={() => void catalog.play(track.id, track.recording_id!)}
              >
                {catalog.pending === track.id ? "Saving…" : "Play"}
              </Button>
            ) : comparison ? (
              <Button
                type="button"
                disabled={catalog.pending !== null}
                aria-label={`Find audio for ${track.title}`}
                onClick={() =>
                  void catalog.findAndPlay(track.id, search(track))
                }
              >
                {catalog.pending === track.id
                  ? "Finding audio…"
                  : "Find audio and play"}
              </Button>
            ) : null}
            {failure && <p role="alert">{failure.message}</p>}
          </>
        )}
      </li>
    );
  }

  return (
    <section aria-label="Album edition track list" aria-busy={pending}>
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
          <h5>{comparison.edition.title}</h5>
          {comparison.is_album_release && (
            <p>This is the release this album was saved from.</p>
          )}
          <p>
            Catalog tracks are saved to your library only when you play them.
            Tracks that are not saved can look for audio in your approved
            providers.
          </p>
          {comparison.edition.media.map((medium) => (
            <section
              key={medium.position}
              aria-label={`Disc ${medium.position}`}
            >
              <h6>Disc {medium.position}</h6>
              <p>{medium.title ?? "Disc title not provided"}</p>
              <ol aria-label={`Disc ${medium.position} tracks`}>
                {medium.tracks.map(row)}
              </ol>
            </section>
          ))}
          {comparison.library_only.length > 0 && (
            <section aria-label="Not in this edition">
              <h6>In your album, not in this edition</h6>
              <ul aria-label="In this album only">
                {comparison.library_only.map((track) => (
                  <li key={track.entry_id}>
                    <p>{track.title}</p>
                    <Button
                      type="button"
                      aria-label={`Play ${track.title}`}
                      onClick={() => play(track)}
                    >
                      Play
                    </Button>
                  </li>
                ))}
              </ul>
            </section>
          )}
        </>
      )}
    </section>
  );
}
