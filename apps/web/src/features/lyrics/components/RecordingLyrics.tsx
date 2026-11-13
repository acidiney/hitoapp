import { useRef, useState } from "react";
import Button from "../../../shared/ui/Button";
import type { useRecordingLyrics } from "../hooks/useRecordingLyrics";
import { accessMessage } from "../model/lyricFailure";
import { identityNames, identityKey } from "../model/lyricIdentity";
import PluginSettingsLink from "../../plugins/components/PluginSettingsLink";
import LyricChangeDialog from "./LyricChangeDialog";
import LyricFinder from "./LyricFinder";
import LyricLookup, { LyricStateCard } from "./LyricLookup";
import LyricPanel from "./LyricPanel";

type SavedFacts = {
  title: string;
  artist: string;
  album: string | null | undefined;
  durationMs: number | null | undefined;
};

function Skeleton() {
  return (
    <LyricStateCard label="Looking up lyrics…" tone="checking" busy>
      <div className="lyric-skeleton" aria-hidden="true">
        <span />
        <span />
        <span />
        <span />
        <span />
      </div>
      <p className="lyric-card-note">
        The music never waits for lyrics. Skeleton lines keep the layout still.
      </p>
    </LyricStateCard>
  );
}

export default function RecordingLyrics({
  recording,
  lyrics,
  saved,
}: {
  recording: string;
  lyrics: ReturnType<typeof useRecordingLyrics>;
  saved: SavedFacts;
}) {
  const [finding, setFinding] = useState(false);
  const [changing, setChanging] = useState(false);
  const opener = useRef<HTMLElement | null>(null);
  const { identities, reader, finder, lookup } = lyrics;
  const items = identities.items;
  const outcome = reader.outcome;
  const view = outcome?.kind === "lyrics" ? outcome.view : null;
  const selected = identities.selected;

  if (lyrics.lost)
    return (
      <section className="track-lyrics" aria-label="Lyrics">
        <LyricStateCard
          label="Couldn't check"
          tone="failed"
          alert
          title={accessMessage}
        >
          <div className="lyric-card-actions">
            <Button onClick={lyrics.retryAfterAccessLoss}>Retry</Button>
          </div>
        </LyricStateCard>
      </section>
    );

  const names = items ? identityNames(items) : {};
  const sourceName = selected ? names[identityKey(selected)] : null;
  const change = (
    <button
      type="button"
      className="lyric-action"
      onClick={(event) => {
        opener.current = event.currentTarget;
        setChanging(true);
      }}
    >
      Change
    </button>
  );

  const startFinding = () => {
    lookup.retire();
    setFinding(true);
  };

  const findFromChange = (
    <button type="button" className="lyric-action" onClick={startFinding}>
      Change
    </button>
  );

  const tryAnother = (
    <button
      type="button"
      className="lyric-action"
      onClick={(event) => {
        opener.current = event.currentTarget;
        setChanging(true);
      }}
    >
      Try another plugin
    </button>
  );

  const body = () => {
    if (!items) return null;
    if (items.length === 0) {
      if (lookup.outcome?.kind === "lyrics")
        return (
          <LyricPanel
            recording={recording}
            view={lookup.outcome.view}
            durationMs={saved.durationMs ?? 0}
            sourceName={lookup.outcome.label}
            change={findFromChange}
          />
        );
      return lookup.engaged ? (
        <LyricLookup
          pending={lookup.pending}
          failure={lookup.failure}
          outcome={lookup.outcome}
          onRetry={lookup.run}
          onFind={startFinding}
        />
      ) : (
        <LyricStateCard
          label="No lyrics saved"
          tone="none"
          title="Find a match to see lyrics here"
        >
          <div className="lyric-card-actions">
            <Button onClick={startFinding}>Find lyrics</Button>
          </div>
        </LyricStateCard>
      );
    }
    if (!selected)
      return (
        <LyricStateCard
          label="Service unavailable"
          tone="failed"
          title="These lyric services are unavailable"
        >
          <div className="lyric-card-actions">{change}</div>
        </LyricStateCard>
      );
    if (reader.pending) return <Skeleton />;
    if (reader.failure || outcome?.kind === "unavailable")
      return (
        <LyricStateCard
          label="Couldn't check"
          tone="failed"
          alert
          title={
            reader.failure &&
            ["changed", "conflict", "gone", "credentials"].includes(
              reader.failure.kind,
            )
              ? reader.failure.message
              : `${sourceName ?? "The lyrics service"} didn't answer`
          }
        >
          <div className="lyric-card-actions">
            <Button onClick={reader.load}>Retry</Button>
            {reader.failure?.kind === "credentials" && <PluginSettingsLink />}
            {tryAnother}
          </div>
        </LyricStateCard>
      );
    if (outcome?.kind === "not_found" || outcome?.kind === "unsupported")
      return (
        <LyricStateCard
          label="None found"
          tone="none"
          title={
            outcome.kind === "unsupported"
              ? "This service can't supply lyrics in a format Hito can read"
              : "No lyrics for this track yet"
          }
        >
          <div className="lyric-card-actions">
            <Button onClick={reader.load}>Look up again</Button>
            {tryAnother}
          </div>
        </LyricStateCard>
      );
    if (view)
      return (
        <LyricPanel
          recording={recording}
          view={view}
          durationMs={saved.durationMs ?? 0}
          sourceName={sourceName}
          change={change}
        />
      );
    return (
      <section className="lyric-panel" aria-label="Lyrics">
        <div className="lyric-source">
          <span>Lyrics{sourceName ? ` · from ${sourceName}` : ""}</span>
          {change}
        </div>
        <p className="lyric-card-note">Lyrics load when you ask.</p>
        <div className="lyric-card-actions">
          <Button onClick={reader.load}>Show lyrics</Button>
        </div>
      </section>
    );
  };

  return (
    <section
      className="track-lyrics"
      aria-label="Lyrics"
      aria-busy={reader.pending || identities.loading}
    >
      {identities.loading && !items && <Skeleton />}
      {identities.failure && (
        <LyricStateCard
          label="Couldn't check"
          tone="failed"
          alert
          title={identities.failure.message}
        >
          <div className="lyric-card-actions">
            <Button onClick={identities.reload}>Retry</Button>
          </div>
        </LyricStateCard>
      )}
      {finder.notice && <p role="status">{finder.notice}</p>}
      {body()}
      {items && finding && (
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
      {items && changing && items.length > 0 && (
        <LyricChangeDialog
          choices={{
            items,
            selected,
            atLimit: identities.atLimit,
            removing: identities.removing,
            removalFailure: identities.removalFailure,
            onSelect: identities.select,
            onRemove: identities.remove,
          }}
          opener={opener}
          onFind={() => {
            setChanging(false);
            startFinding();
          }}
          onClose={() => setChanging(false)}
        />
      )}
    </section>
  );
}
