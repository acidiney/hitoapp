import type { Schema } from "../../../shared/schema";
import Artwork from "../../artwork/components/Artwork";
import type { RadioState } from "../hooks/useArtistRadio";

export default function RadioStarts({
  artists,
  radio,
  onStart,
}: {
  artists: Schema<"ArtistIdentityGroup">[];
  radio: RadioState;
  onStart: (artist: Schema<"ArtistIdentityGroup">) => void;
}) {
  if (artists.length === 0) return null;
  return (
    <section className="discover-section" aria-labelledby="discover-radio">
      <h2 id="discover-radio">Radio from…</h2>
      <ul className="radio-starts">
        {artists.map((artist) => {
          const busy = radio.pending === artist.id;
          return (
            <li key={artist.id}>
              <button
                type="button"
                className="radio-start"
                aria-label={`Start radio from ${artist.name.value}`}
                aria-busy={busy || undefined}
                disabled={radio.pending !== null}
                onClick={() => onStart(artist)}
              >
                <span className="radio-start-cover" aria-hidden="true">
                  <Artwork
                    recording={artist.name.recording_id}
                    fallback={{ id: artist.id, title: artist.name.value }}
                  />
                </span>
                <span className="radio-start-copy">
                  <span className="radio-start-name">{artist.name.value}</span>
                  <span className="radio-start-why">
                    {busy
                      ? "Building your radio…"
                      : `${artist.recording_count} ${artist.recording_count === 1 ? "track" : "tracks"} in your library`}
                  </span>
                </span>
              </button>
            </li>
          );
        })}
      </ul>
      {radio.notice &&
        (radio.notice.tone === "error" ? (
          <p className="radio-notice" role="alert" data-tone="error">
            {radio.notice.text}
          </p>
        ) : (
          <p className="radio-notice" role="status">
            {radio.notice.text}
          </p>
        ))}
    </section>
  );
}
