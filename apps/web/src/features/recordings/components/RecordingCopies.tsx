import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";
import AvailabilityLabel from "../../../shared/ui/AvailabilityLabel";
import Button from "../../../shared/ui/Button";

const PROOF: Record<NonNullable<Schema<"RecordingCopy">["proof"]>, string> = {
  musicbrainz_recording: "Same MusicBrainz recording in its tags",
  isrc: "Same ISRC",
};

export default function RecordingCopies({
  recording,
  copies,
  splitting,
  error,
  onSplit,
}: {
  recording: string;
  copies: Schema<"RecordingCopy">[];
  splitting: string;
  error: string;
  onSplit: (copy: string) => void;
}) {
  if (copies.length === 0 && !error) return null;
  return (
    <section className="rd-card" aria-labelledby="recording-copies-heading">
      <h2 id="recording-copies-heading">Copies of this song</h2>
      {copies.length > 0 && (
        <p>
          Hito can prove these are the same song, so your lists show one card.
          Every copy keeps its own audio, likes and playlists.
        </p>
      )}
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      <ul className="rd-rows">
        {copies.map((copy) => (
          <li className="rd-row" key={copy.recording_id}>
            <div>
              {copy.recording_id === recording ? (
                <strong>{copy.title}</strong>
              ) : (
                <Link
                  className="text-link"
                  href={`/recordings/${encodeURIComponent(copy.recording_id)}`}
                >
                  {copy.title}
                </Link>
              )}
              <span>
                {" "}
                · {copy.artist}
                {copy.album ? ` · ${copy.album}` : ""}
              </span>
              <div>
                <AvailabilityLabel
                  state={copy.available ? "available" : "catalog"}
                  label={copy.available ? "Audio ready" : "No audio ready"}
                />{" "}
                {copy.proof ? PROOF[copy.proof] : "Shown in your lists"}
              </div>
              {copy.sources.length > 0 && (
                <ul>
                  {copy.sources.map((source) => (
                    <li key={source.id}>
                      {source.label}
                      {source.ready ? " · ready" : ""}
                    </li>
                  ))}
                </ul>
              )}
            </div>
            {!copy.canonical && (
              <Button
                disabled={splitting !== ""}
                onClick={() => onSplit(copy.recording_id)}
              >
                {splitting === copy.recording_id
                  ? "Separating…"
                  : "These are different songs"}
              </Button>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}
