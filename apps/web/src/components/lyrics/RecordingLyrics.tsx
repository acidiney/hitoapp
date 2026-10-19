import type { LyricsLookup } from "../../lyricsApi";
import RecordingCredits from "../metadata/RecordingCredits";

type Props = {
  available: boolean;
  pending: boolean;
  result: LyricsLookup | null;
  error: string;
  refresh: boolean;
  onLoad: () => void;
  onCancel: () => void;
  onRefreshDetails: () => void;
};

export default function RecordingLyrics({
  available,
  pending,
  result,
  error,
  refresh,
  onLoad,
  onCancel,
  onRefreshDetails,
}: Props) {
  const lyrics = result?.status === "ready" ? result.document.lyrics : null;
  return (
    <section aria-label="Lyrics" aria-busy={pending}>
      <h2>Lyrics</h2>
      {!available && <p>Lyrics are not available for this track yet.</p>}
      {available && (
        <>
          {pending ? (
            <>
              <p role="status">Loading lyrics…</p>
              <button type="button" className="button" onClick={onCancel}>
                Cancel lyric request
              </button>
            </>
          ) : (
            <button type="button" className="button" onClick={onLoad}>
              {result || error ? "Retry lyrics" : "Show lyrics"}
            </button>
          )}
          {error && <p role="alert">{error}</p>}
          {refresh && (
            <button type="button" className="button" onClick={onRefreshDetails}>
              Refresh track details
            </button>
          )}
          {result?.status === "unsupported" && (
            <p>This provider does not offer lyrics for this track.</p>
          )}
          {result?.status === "unavailable" && (
            <p>Lyrics are unavailable right now. Try again.</p>
          )}
          {result?.status === "cancelled" && (
            <p>The lyric request was cancelled.</p>
          )}
          {result?.status === "ready" && !lyrics && (
            <p>No lyrics found for this recording.</p>
          )}
          {lyrics && (
            <div>
              {lyrics.rendition_label && <h3>{lyrics.rendition_label}</h3>}
              <ol aria-label="Lyric lines">
                {lyrics.lines.map((line, index) => (
                  <li key={index}>{line.text}</li>
                ))}
              </ol>
              <RecordingCredits attribution={lyrics.attribution} />
            </div>
          )}
        </>
      )}
    </section>
  );
}
