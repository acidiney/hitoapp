import type { Schema } from "../../../shared/schema";
import Button from "../../../shared/ui/Button";
import { time } from "../../../shared/format";

export default function AudiobookBookmarkList({
  detail,
  disabled,
  remove,
  play,
  playbackPending,
}: {
  detail: Schema<"AudiobookDetail">;
  disabled: boolean;
  remove: (id: string) => Promise<void>;
  play: (
    bookmark: Schema<"AudiobookDetail">["bookmarks"][number],
  ) => Promise<void>;
  playbackPending: boolean;
}) {
  const chapters = new Map(
    detail.chapters.map(
      (entry, index) => [entry.entry_id, { entry, number: index + 1 }] as const,
    ),
  );
  if (detail.bookmarks.length === 0)
    return <p className="media-side-note">No bookmarks yet.</p>;
  return (
    <ol className="bookmark-list">
      {detail.bookmarks.map((bookmark) => {
        const chapter = chapters.get(bookmark.entry_id);
        const anchored =
          chapter?.entry.recording_id === bookmark.recording_id
            ? chapter
            : undefined;
        return (
          <li key={bookmark.id} className="bookmark-card">
            <span className="media-mono">
              {anchored ? `Ch. ${anchored.number} · ` : ""}
              {time(bookmark.position_ms / 1000)}
            </span>
            <strong>{bookmark.label}</strong>
            <p className="media-side-note">
              {anchored
                ? `Chapter ${anchored.number}: ${anchored.entry.title || "Untitled chapter"}`
                : "Chapter unavailable"}
            </p>
            {anchored && !anchored.entry.available && (
              <p className="media-side-note">Chapter unavailable</p>
            )}
            <div className="bookmark-actions">
              {anchored?.entry.available && (
                <Button
                  disabled={disabled || playbackPending}
                  onClick={() => void play(bookmark)}
                >
                  Play bookmark
                </Button>
              )}
              <Button
                disabled={disabled}
                onClick={() => void remove(bookmark.id)}
              >
                Remove bookmark
              </Button>
            </div>
          </li>
        );
      })}
    </ol>
  );
}
