import type { ReactNode } from "react";
import { followableLines, type LyricView } from "../model/lyricContent";
import FollowedLyrics from "./FollowedLyrics";
import LyricText, { LyricCredits } from "./LyricText";

export default function LyricPanel({
  recording,
  view,
  durationMs,
  sourceName,
  change,
}: {
  recording: string;
  view: LyricView;
  durationMs: number;
  sourceName: string | null;
  change: ReactNode;
}) {
  const followed = followableLines(view, durationMs);
  return (
    <section
      className="lyric-panel"
      aria-label={followed ? "Synced lyrics" : "Lyrics without timing"}
      data-synced={followed ? "" : undefined}
    >
      <div className="lyric-source">
        <span>
          {followed ? "Synced" : "Not synced"}
          {sourceName ? ` · from ${sourceName}` : ""}
        </span>
        {change}
      </div>
      {followed ? (
        <>
          <FollowedLyrics recording={recording} lines={followed} />
          <LyricCredits view={view} />
        </>
      ) : (
        <LyricText view={view} />
      )}
    </section>
  );
}
