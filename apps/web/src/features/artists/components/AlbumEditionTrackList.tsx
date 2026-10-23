import { time } from "../../../shared/format";
import type {
  AlbumEditionTrackListAvailability,
  Recording,
} from "../../../shared/types";
import { usePlayer } from "../../playback/components/PlaybackProvider";
import { useCatalogTrackPlay } from "../hooks/useCatalogTrackPlay";

type Comparison = Extract<
  AlbumEditionTrackListAvailability,
  { status: "available" }
>;
type Occurrence = Comparison["edition"]["media"][number]["tracks"][number];

const kinds = { audio: "Audio", pregap: "Pregap", data: "Data" };

export default function AlbumEditionTrackList({
  account,
  comparison,
  pending,
  error,
  retry,
}: {
  account: string;
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
  );
  const linked = new Map<string, Recording>(
    comparison?.matches.map((match) => [
      match.occurrence_id,
      match.recording,
    ]) ?? [],
  );
  const play = (recording: Recording) => void player.playQueue([recording], 0);

  function row(track: Occurrence) {
    const library =
      linked.get(track.id) ??
      (track.recording_id ? catalog.saved.get(track.recording_id) : undefined);
    const failure =
      catalog.failed?.id === track.recording_id ? catalog.failed : null;
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
            <button
              type="button"
              className="button"
              aria-label={`Play ${library.title}`}
              onClick={() => play(library)}
            >
              Play
            </button>
          </>
        ) : track.kind !== "audio" ? (
          <p>Not playable audio</p>
        ) : (
          <>
            <p>Not in your library</p>
            {track.recording_id == null ? (
              <p>No provider identity for this track</p>
            ) : comparison?.adoption ? (
              <button
                type="button"
                className="button"
                disabled={catalog.pending !== null}
                aria-label={`Play ${track.title}`}
                onClick={() => void catalog.play(track.recording_id!)}
              >
                {catalog.pending === track.recording_id ? "Saving…" : "Play"}
              </button>
            ) : (
              <p>No connected provider can save this track</p>
            )}
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
          <button
            type="button"
            className="button"
            disabled={pending}
            onClick={() => void retry()}
          >
            Try track list again
          </button>
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
                    <button
                      type="button"
                      className="button"
                      aria-label={`Play ${track.title}`}
                      onClick={() => play(track)}
                    >
                      Play
                    </button>
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
