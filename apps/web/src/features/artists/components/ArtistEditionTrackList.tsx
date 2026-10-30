import type { Schema } from "../../../shared/schema";
import { time } from "../../../shared/format";

import Button from "../../../shared/ui/Button";

const kinds = { audio: "Audio", pregap: "Pregap", data: "Data" };

export default function ArtistEditionTrackList({
  edition,
  pending,
  error,
  retry,
}: {
  edition: Schema<"CatalogEditionTrackList"> | null;
  pending: boolean;
  error: string;
  retry: () => Promise<void>;
}) {
  return (
    <section
      aria-label="Selected edition track list"
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
      {edition && (
        <>
          <h5>{edition.title}</h5>
          <p className="edition-credit">
            {edition.artist_credit?.length
              ? edition.artist_credit
                  .map((member) => member.credited_name + member.join_phrase)
                  .join("")
              : "Artist credit not provided"}
          </p>
          <p className="edition-note">
            Audio availability has not been checked.
          </p>
          {edition.media.map((medium) => (
            <section
              key={medium.position}
              aria-label={`Disc ${medium.position}`}
            >
              <div
                className={edition.media.length > 1 ? "disc-header" : "sr-only"}
              >
                <h6>Disc {medium.position}</h6>
                <p className={medium.title ? "" : "placeholder"}>
                  {medium.title ?? "Disc title not provided"}
                </p>
                <p className={medium.format ? "meta" : "placeholder"}>
                  {medium.format ?? "Format not provided"}
                </p>
              </div>
              <ol
                aria-label={`Disc ${medium.position} tracks`}
                className="edition-track-rows"
              >
                {medium.tracks.map((track) => (
                  <li key={track.id}>
                    <p
                      className={
                        track.number ? "track-no" : "track-no placeholder"
                      }
                    >
                      {track.number ?? "Track number not provided"}
                    </p>
                    <p className="track-name">{track.title}</p>
                    {track.kind !== "audio" && (
                      <p className="track-kind">{kinds[track.kind]}</p>
                    )}
                    <p className="track-credit">
                      {track.artist_credit?.length
                        ? track.artist_credit
                            .map(
                              (member) =>
                                member.credited_name + member.join_phrase,
                            )
                            .join("")
                        : "Artist credit not provided"}
                    </p>
                    <p
                      className={
                        track.duration_ms == null
                          ? "track-time placeholder"
                          : "track-time"
                      }
                    >
                      {track.duration_ms == null
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
