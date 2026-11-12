import type { ReactNode } from "react";
import Button from "../../../shared/ui/Button";
import { Link } from "@inertiajs/react";
import type { AutomaticOutcome } from "../model/lyricContent";
import type { LyricFailure } from "../model/lyricFailure";
import LyricState, { type LyricStateTone } from "./LyricState";

export type LyricLookupProps = {
  pending: boolean;
  failure: LyricFailure | null;
  outcome: AutomaticOutcome | null;
  onRetry: () => void;
  onFind: () => void;
};

export function LyricStateCard({
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
  failure,
  outcome,
  onRetry,
  onFind,
}: LyricLookupProps) {
  if (pending)
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
          The music never waits for lyrics. Skeleton lines keep the layout
          still.
        </p>
      </LyricStateCard>
    );
  if (failure)
    return (
      <LyricStateCard
        label="Couldn't check"
        tone="failed"
        alert
        title={failure.message}
      >
        <div className="lyric-card-actions">
          <Button onClick={onRetry}>Retry</Button>
        </div>
      </LyricStateCard>
    );
  const kind = outcome?.kind;
  if (kind === "unavailable")
    return (
      <LyricStateCard
        label="Couldn't check"
        tone="failed"
        alert
        title="The lyrics service didn't answer"
      >
        <div className="lyric-card-actions">
          <Button onClick={onRetry}>Retry</Button>
        </div>
      </LyricStateCard>
    );
  if (kind === "no_services")
    return (
      <LyricStateCard
        label="No lyrics service"
        tone="none"
        title="No lyrics service is connected"
      >
        <div className="lyric-card-actions">
          <Link href="/settings?section=plugins">Connect a lyrics service</Link>
        </div>
      </LyricStateCard>
    );
  if (kind === "ambiguous")
    return (
      <LyricStateCard
        label="Several matches"
        tone="none"
        title="More than one lyric result could fit this track"
      >
        <div className="lyric-card-actions">
          <Button onClick={onFind}>Choose</Button>
        </div>
      </LyricStateCard>
    );
  return (
    <LyricStateCard
      label="None found"
      tone="none"
      title={
        kind === "unsupported"
          ? "This service can't supply lyrics in a format Hito can read"
          : "No lyrics for this track yet"
      }
    >
      <div className="lyric-card-actions">
        <Button onClick={onFind}>Find lyrics</Button>
      </div>
    </LyricStateCard>
  );
}
