import { useState, type ReactNode } from "react";
import Button from "../../../shared/ui/Button";
import type { CompatibleChoice } from "../api/compatibleLyrics";
import type { SelectedLyricsOutcome } from "../model/lyricContent";
import type { LyricFailure } from "../model/lyricFailure";
import LyricState, { type LyricStateTone } from "./LyricState";
import LyricText from "./LyricText";

export type LyricLookupProps = {
  pending: boolean;
  outcome: SelectedLyricsOutcome | null;
  failure: LyricFailure | null;
  choices: CompatibleChoice[];
  chosenKey: string | null;
  onLookup: (key?: string) => void;
};

function Card({
  label,
  tone,
  title,
  busy,
  alert,
  children,
}: {
  label: string;
  tone: LyricStateTone;
  title?: string;
  busy?: boolean;
  alert?: boolean;
  children?: ReactNode;
}) {
  return (
    <section
      className="lyric-card"
      aria-label={label}
      aria-busy={busy || undefined}
      role={alert ? "alert" : undefined}
    >
      <LyricState tone={tone}>{label}</LyricState>
      {title && <p className="lyric-card-title">{title}</p>}
      {children}
    </section>
  );
}

export default function LyricLookup({
  pending,
  outcome,
  failure,
  choices,
  chosenKey,
  onLookup,
}: LyricLookupProps) {
  const [changing, setChanging] = useState(false);
  if (pending)
    return (
      <Card label="Looking up lyrics…" tone="checking" busy>
        <div className="lyric-skeleton" aria-hidden="true">
          <span />
          <span />
          <span />
          <span />
          <span />
        </div>
        <p className="lyric-card-note">
          The music never waits for lyrics. Skeleton lines keep the layout
          still.
        </p>
      </Card>
    );
  const failed = failure !== null || outcome?.kind === "unavailable";
  if (failed)
    return (
      <Card
        label="Couldn't check"
        tone="failed"
        alert
        title={failure?.message ?? "Lyrics are unavailable right now."}
      >
        <div className="lyric-card-actions">
          <Button onClick={() => onLookup()}>Retry</Button>
        </div>
      </Card>
    );
  if (outcome?.kind === "lyrics") {
    const chosen = choices.find((item) => item.key === chosenKey);
    return (
      <>
        <div className="lyric-source">
          <span>
            {outcome.view.synced ? "Synced" : "Not synced"}
            {chosen ? ` · from ${chosen.label}` : ""}
          </span>
          {choices.length > 1 && (
            <button
              type="button"
              className="text-link"
              aria-expanded={changing}
              onClick={() => setChanging(!changing)}
            >
              Change
            </button>
          )}
        </div>
        {changing && (
          <div
            className="lyric-card-actions"
            role="group"
            aria-label="Lyric provider"
          >
            {choices.map((item) => (
              <Button
                key={item.key}
                variant={item.key === chosenKey ? "active" : "secondary"}
                onClick={() => {
                  setChanging(false);
                  onLookup(item.key);
                }}
              >
                {item.label}
              </Button>
            ))}
          </div>
        )}
        <LyricText view={outcome.view} />
      </>
    );
  }
  if (outcome?.kind === "not_found" || outcome?.kind === "unsupported")
    return (
      <Card
        label="None found"
        tone="none"
        title={
          outcome.kind === "unsupported"
            ? "This provider has no lyrics for this track."
            : "No lyrics for this track yet"
        }
      >
        <div className="lyric-card-actions">
          <Button onClick={() => onLookup()}>Look up again</Button>
        </div>
      </Card>
    );
  return null;
}
