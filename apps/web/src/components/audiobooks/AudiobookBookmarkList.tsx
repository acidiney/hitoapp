import { time } from "../../shared/format";
import type { AudiobookDetail } from "../../shared/types";

export default function AudiobookBookmarkList({
  detail,
  disabled,
  remove,
  play,
  playbackPending,
}: {
  detail: AudiobookDetail;
  disabled: boolean;
  remove: (id: string) => Promise<void>;
  play: (bookmark: AudiobookDetail["bookmarks"][number]) => Promise<void>;
  playbackPending: boolean;
}) {
  const chapters = new Map(
    detail.chapters.map(
      (entry, index) => [entry.entry_id, { entry, number: index + 1 }] as const,
    ),
  );
  if (detail.bookmarks.length === 0) return <p>No bookmarks yet.</p>;
  return (
    <ol>
      {detail.bookmarks.map((bookmark) => {
        const chapter = chapters.get(bookmark.entry_id);
        const anchored =
          chapter?.entry.recording_id === bookmark.recording_id
            ? chapter
            : undefined;
        return (
          <li key={bookmark.id}>
            <strong>{bookmark.label}</strong>
            <p>
              {anchored
                ? `Chapter ${anchored.number}: ${anchored.entry.title || "Untitled chapter"}`
                : "Chapter unavailable"}
            </p>
            <span>{time(bookmark.position_ms / 1000)}</span>
            {anchored && !anchored.entry.available && (
              <p>Chapter unavailable</p>
            )}
            {anchored?.entry.available && (
              <button
                className="button"
                type="button"
                disabled={disabled || playbackPending}
                onClick={() => void play(bookmark)}
              >
                Play bookmark
              </button>
            )}
            <button
              className="button"
              type="button"
              disabled={disabled}
              onClick={() => void remove(bookmark.id)}
            >
              Remove bookmark
            </button>
          </li>
        );
      })}
    </ol>
  );
}
