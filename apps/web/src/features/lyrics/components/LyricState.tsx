import type { ReactNode } from "react";

export type LyricStateTone = "checking" | "none" | "failed";

export default function LyricState({
  tone,
  children,
}: {
  tone: LyricStateTone;
  children: ReactNode;
}) {
  return (
    <span className="source-status lyric-state" data-status={tone}>
      <span className="source-status-dot" aria-hidden="true" />
      {children}
    </span>
  );
}
