import { useState } from "react";
import { RefreshCw } from "lucide-react";
import Button from "../../../shared/ui/Button";
import type { useRecordingLyrics } from "../hooks/useRecordingLyrics";
import { lyricLanguageName } from "../model/lyricLanguage";
import LyricChoices from "./LyricChoices";
import LyricFinder from "./LyricFinder";
import LyricLookup from "./LyricLookup";
import LyricState from "./LyricState";
import LyricText from "./LyricText";

type SavedFacts = {
  title: string;
  artist: string;
  album: string | null | undefined;
  durationMs: number | null | undefined;
};

export default function RecordingLyrics({
  lyrics,
  saved,
}: {
  lyrics: ReturnType<typeof useRecordingLyrics>;
  saved: SavedFacts;
}) {
  const [finding, setFinding] = useState(false);
  const { identities, reader, finder, lookup } = lyrics;
  const items = identities.items;
  const outcome = reader.outcome;
  const view = outcome?.kind === "lyrics" ? outcome.view : null;
  const selected = identities.selected;

  if (lyrics.lost)
    return (
      <section className="track-lyrics" aria-label="Lyrics">
        <p role="alert">
          Your account access changed. Sign in again to use lyrics.
        </p>
        <Button onClick={lyrics.retryAfterAccessLoss}>Try again</Button>
      </section>
    );

  return (
    <section
      className="track-lyrics"
      aria-label="Lyrics"
      aria-busy={reader.pending || identities.loading}
    >
      {identities.loading && !items && (
        <p role="status">
          <LyricState tone="checking">Loading lyric choices…</LyricState>
        </p>
      )}
      {identities.failure && (
        <>
          <p role="alert">{identities.failure.message}</p>
          <Button onClick={identities.reload}>Try again</Button>
        </>
      )}
      {items && (
        <div className="track-toolbar">
          {view && (
            <span
              className="track-chip"
              data-tone={view.synced ? "success" : "neutral"}
            >
              {view.synced ? "Synced" : "Unsynced"}
            </span>
          )}
          {view?.language && (
            <span className="track-chip">
              {lyricLanguageName(view.language)}
            </span>
          )}
          {outcome?.kind === "not_found" && (
            <span className="track-chip" data-tone="warning">
              No lyrics
            </span>
          )}
        </div>
      )}
      {items && items.length === 0 && !lookup.engaged && (
        <p className="track-muted">No lyrics are saved for this track yet.</p>
      )}
      {items && items.length === 0 && lookup.engaged && (
        <LyricLookup
          pending={lookup.pending}
          outcome={lookup.outcome}
          failure={lookup.failure}
          choices={lookup.choices}
          chosenKey={lookup.chosenKey}
          onLookup={lookup.run}
        />
      )}
      {items && items.length > 0 && (
        <LyricChoices
          items={items}
          selected={selected}
          atLimit={identities.atLimit}
          removing={identities.removing}
          removalFailure={identities.removalFailure}
          onSelect={identities.select}
          onRemove={identities.remove}
        />
      )}
      {finder.notice && <p role="status">{finder.notice}</p>}
      {items && selected && (
        <div className="track-toolbar">
          {reader.pending ? (
            <Button onClick={reader.cancel}>Cancel lyric request</Button>
          ) : (
            <Button variant="primary" onClick={reader.load}>
              <RefreshCw size={14} aria-hidden="true" />
              {outcome || reader.failure ? "Retry lyrics" : "Show lyrics"}
            </Button>
          )}
        </div>
      )}
      {reader.pending && (
        <p role="status">
          <LyricState tone="checking">Loading lyrics…</LyricState>
        </p>
      )}
      {reader.failure && (
        <>
          <p role="alert">
            <LyricState tone="failed">Couldn't check</LyricState>{" "}
            {reader.failure.message}
          </p>
        </>
      )}
      {outcome?.kind === "unsupported" && (
        <p className="track-empty">
          This connected service can't supply lyrics in a format Hito can read.
        </p>
      )}
      {outcome?.kind === "unavailable" && (
        <p className="track-empty" role="alert">
          <LyricState tone="failed">Couldn't check</LyricState> Lyrics are
          unavailable right now. Try again.
        </p>
      )}
      {outcome?.kind === "not_found" && (
        <p className="track-empty">
          <LyricState tone="none">None found</LyricState> No lyrics found for
          this choice. It is kept, so you can also find lyrics from another
          match.
        </p>
      )}
      {view && <LyricText view={view} />}
      {items && (
        <>
          <Button
            variant={items.length === 0 ? "primary" : "secondary"}
            aria-expanded={finding}
            onClick={() => setFinding(!finding)}
          >
            Find lyrics
          </Button>
          {finding && (
            <LyricFinder
              services={finder.services}
              servicesFailure={finder.servicesFailure}
              service={finder.service}
              query={finder.query}
              search={finder.search}
              review={{
                saved,
                candidate: finder.review.candidate,
                phase: finder.review.phase,
                review: finder.review.review,
                failure: finder.review.failure,
                duplicate: finder.duplicate,
                atLimit: identities.atLimit,
                onAccept: finder.accept,
                onReviewAgain: () =>
                  finder.review.candidate &&
                  void finder.review.choose(finder.review.candidate),
                onClose: finder.review.clear,
              }}
              onService={finder.chooseService}
              onQuery={finder.setQuery}
              onSearch={() => void finder.search.search(finder.query)}
              onCancelSearch={finder.search.cancel}
              onReview={(candidate) => void finder.review.choose(candidate)}
            />
          )}
        </>
      )}
    </section>
  );
}
