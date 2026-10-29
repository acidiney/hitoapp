import { Play } from "lucide-react";
import { time } from "../../../shared/format";
import type { PlaylistEntry } from "../../../shared/types";

export default function AudiobookChapters({
  chapters,
  currentEntry,
  currentPositionMs,
  disabled,
  play,
}: {
  chapters: PlaylistEntry[];
  currentEntry: string | null;
  currentPositionMs: number | null;
  disabled: boolean;
  play: (chapter: PlaylistEntry) => Promise<void>;
}) {
  return (
    <ol className="chapter-list" aria-label="Chapters">
      {chapters.map((chapter, index) => {
        const playable = chapter.recording_id != null && chapter.available;
        const current = chapter.entry_id === currentEntry;
        return (
          <li
            key={chapter.entry_id}
            id={`chapter-${chapter.entry_id}`}
            className="chapter-row"
            data-current={current || undefined}
            data-unavailable={!playable || undefined}
          >
            <span className="chapter-number">{index + 1}</span>
            <span className="chapter-title">
              {chapter.title || "Untitled chapter"}
            </span>
            <span className="chapter-state" data-current={current || undefined}>
              {!playable
                ? "Chapter unavailable"
                : current && currentPositionMs != null
                  ? `Listening · ${time(currentPositionMs / 1000)}`
                  : ""}
            </span>
            <span className="chapter-length">
              {chapter.duration_ms > 0 ? time(chapter.duration_ms / 1000) : "—"}
            </span>
            {playable ? (
              <button
                type="button"
                className="round-button"
                aria-label={`Play chapter ${index + 1} from beginning`}
                title="Play from the beginning"
                disabled={disabled}
                onClick={() => void play(chapter)}
              >
                <Play size={14} fill="currentColor" />
              </button>
            ) : (
              <span />
            )}
          </li>
        );
      })}
    </ol>
  );
}
