import type { Schema } from "../../../shared/schema";
import { RefreshCw } from "lucide-react";

import RecordingCredits from "../../metadata/components/RecordingCredits";
import Button from "../../../shared/ui/Button";
import { time } from "../../../shared/format";

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

function languageName(code: string) {
  try {
    return new Intl.DisplayNames(["en"], { type: "language" }).of(code) ?? code;
  } catch {
    return code;
  }
}

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
  const synced = lyrics?.lines.some((line) => line.start_ms > 0) ?? false;
  const none = result?.status === "ready" && !lyrics;
  return (
    <section
      className="track-lyrics"
      aria-label="Lyrics"
      aria-busy={pending || providers.pending}
    >
      <div className="track-toolbar">
        {lyrics && (
          <span
            className="track-chip"
            data-tone={synced ? "success" : "neutral"}
          >
            {synced ? "Synced" : "Unsynced"}
          </span>
        )}
        {lyrics?.language && (
          <span className="track-chip">{languageName(lyrics.language)}</span>
        )}
        {none && (
          <span className="track-chip" data-tone="warning">
            No lyrics
          </span>
        )}
        {providers.items && providers.items.length > 0 && (
          <label className="track-field track-listbox">
            <span className="sr-only">Lyric provider</span>
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
        {providers.pending ? (
          <Button type="button" onClick={providers.cancel}>
            Cancel provider request
          </Button>
        ) : (
          !providers.items && (
            <Button type="button" onClick={providers.load}>
              Choose lyric provider
            </Button>
          )
        )}
        {available &&
          (pending ? (
            <Button type="button" onClick={onCancel}>
              Cancel lyric request
            </Button>
          ) : (
            <Button type="button" onClick={onLoad}>
              <RefreshCw size={14} aria-hidden="true" />
              {result || error ? "Retry lyrics" : "Show lyrics"}
            </Button>
          ))}
      </div>
      {providers.pending && <p role="status">Loading lyric providers…</p>}
      {providers.error && <p role="alert">{providers.error}</p>}
      {(providers.refresh || (available && refresh)) && (
        <Button type="button" onClick={onRefreshDetails}>
          Refresh track details
        </Button>
      )}
      {providers.items?.length === 0 && (
        <p className="track-muted">
          No connected lyric providers are available for this track.
        </p>
      )}
      {!available &&
        !providers.pending &&
        !providers.items &&
        !providers.error && <p className="track-muted">No lyrics yet.</p>}
      {available && (
        <>
          {pending && <p role="status">Loading lyrics…</p>}
          {error && <p role="alert">{error}</p>}
          {result?.status === "unsupported" && (
            <p className="track-empty">
              This provider does not offer lyrics for this track.
            </p>
          )}
          {result?.status === "unavailable" && (
            <p className="track-empty">
              Lyrics are unavailable right now. Try again.
            </p>
          )}
          {result?.status === "cancelled" && (
            <p className="track-empty">The lyric request was cancelled.</p>
          )}
          {none && (
            <p className="track-empty">No lyrics found for this recording.</p>
          )}
          {lyrics && (
            <div className="track-lyrics-body">
              {lyrics.rendition_label && <h3>{lyrics.rendition_label}</h3>}
              <ol aria-label="Lyric lines" data-synced={synced}>
                {lyrics.lines.map((line, index) => (
                  <li
                    key={index}
                    data-time={synced ? time(line.start_ms / 1000) : undefined}
                  >
                    {line.text}
                  </li>
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
