import { time } from "../../../shared/format";
import type { CatalogEditionTrackList } from "../../../shared/types";

const kinds = { audio: "Audio", pregap: "Pregap", data: "Data" };

export default function ArtistEditionTrackList({
  edition,
  pending,
  error,
  retry,
}: {
  edition: CatalogEditionTrackList | null;
  pending: boolean;
  error: string;
  retry: () => Promise<void>;
}) {
  return (
    <section aria-label="Selected edition track list" aria-busy={pending}>
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
      {edition && (
        <>
          <h5>{edition.title}</h5>
          <p>
            {edition.artist_credit?.length
              ? edition.artist_credit
                  .map((member) => member.credited_name + member.join_phrase)
                  .join("")
              : "Artist credit not provided"}
          </p>
          <p>Audio availability has not been checked.</p>
          {edition.media.map((medium) => (
            <section
              key={medium.position}
              aria-label={`Disc ${medium.position}`}
            >
              <h6>Disc {medium.position}</h6>
              <p>{medium.title ?? "Disc title not provided"}</p>
              <p>{medium.format ?? "Format not provided"}</p>
              <ol aria-label={`Disc ${medium.position} tracks`}>
                {medium.tracks.map((track) => (
                  <li key={track.id}>
                    <p>{track.number ?? "Track number not provided"}</p>
                    <p>{track.title}</p>
                    <p>{kinds[track.kind]}</p>
                    <p>
                      {track.artist_credit?.length
                        ? track.artist_credit
                            .map(
                              (member) =>
                                member.credited_name + member.join_phrase,
                            )
                            .join("")
                        : "Artist credit not provided"}
                    </p>
                    <p>
                      {track.duration_ms === null ||
                      track.duration_ms === undefined
                        ? "Duration not provided"
                        : time(track.duration_ms / 1000)}
                    </p>
                  </li>
                ))}
              </ol>
              {medium.tracks.length === 0 && (
                <p>No track occurrences were provided for this disc.</p>
              )}
            </section>
          ))}
        </>
      )}
    </section>
  );
}
