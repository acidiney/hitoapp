import type { ReactNode } from "react";
import Button from "../../../shared/ui/Button";
import type { LyricFailure } from "../model/lyricFailure";
import LyricState, { type LyricStateTone } from "./LyricState";

export type LyricLookupProps = {
  pending: boolean;
  failure: LyricFailure | null;
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
  return (
    <LyricStateCard
      label="None found"
      tone="none"
      title="No lyrics for this track yet"
    >
      <div className="lyric-card-actions">
        <Button onClick={onFind}>Find lyrics</Button>
      </div>
    </LyricStateCard>
  );
}
