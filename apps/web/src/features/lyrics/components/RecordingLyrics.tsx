import type { Schema } from "../../../shared/schema";

import RecordingCredits from "../../metadata/components/RecordingCredits";
import Button from "../../../shared/ui/Button";

type Props = {
  providers: {
    pending: boolean;
    items: Schema<"RecordingLyricsProvider">[] | null;
    selected: Schema<"RecordingLyricsProvider"> | null;
    error: string;
    refresh: boolean;
    load: () => void;
    select: (observation: string) => void;
    cancel: () => void;
  };
  available: boolean;
  pending: boolean;
  result: Schema<"LyricsLookup"> | null;
  error: string;
  refresh: boolean;
  onLoad: () => void;
  onCancel: () => void;
  onRefreshDetails: () => void;
};

export default function RecordingLyrics({
  providers,
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
    <section
      className="rd-card"
      aria-label="Lyrics"
      aria-busy={pending || providers.pending}
    >
      <h2>Lyrics</h2>
      {providers.pending ? (
        <>
          <p role="status">Loading lyric providers…</p>
          <Button type="button" onClick={providers.cancel}>
            Cancel provider request
          </Button>
        </>
      ) : (
        <Button type="button" onClick={providers.load}>
          Choose lyric provider
        </Button>
      )}
      {providers.items && providers.items.length > 0 && (
        <label className="rd-field">
          Lyric provider
          <select
            value={providers.selected?.observation_id ?? ""}
            onChange={(event) => providers.select(event.target.value)}
          >
            {providers.items.map((provider) => (
              <option
                key={provider.observation_id}
                value={provider.observation_id}
              >
                {provider.label}
              </option>
            ))}
          </select>
        </label>
      )}
      {providers.error && <p role="alert">{providers.error}</p>}
      {providers.refresh && (
        <Button type="button" onClick={onRefreshDetails}>
          Refresh track details
        </Button>
      )}
      {providers.items?.length === 0 && (
        <p>No connected lyric providers are available for this track.</p>
      )}
      {!available &&
        !providers.pending &&
        !providers.items &&
        !providers.error && <p>Lyrics are not available for this track yet.</p>}
      {available && (
        <>
          {pending ? (
            <>
              <p role="status">Loading lyrics…</p>
              <Button type="button" onClick={onCancel}>
                Cancel lyric request
              </Button>
            </>
          ) : (
            <Button type="button" onClick={onLoad}>
              {result || error ? "Retry lyrics" : "Show lyrics"}
            </Button>
          )}
          {error && <p role="alert">{error}</p>}
          {refresh && (
            <Button type="button" onClick={onRefreshDetails}>
              Refresh track details
            </Button>
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
