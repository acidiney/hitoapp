import type { FormEvent } from "react";
import type { PlaylistEntry } from "../../../shared/types";

export default function AudiobookBookmarkForm({
  chapters,
  draft,
  pending,
  hasAttempt,
  acknowledged,
  canCheck,
  error,
  message,
  change,
  save,
  retry,
}: {
  chapters: PlaylistEntry[];
  draft: { entry: string; position: string; label: string };
  pending: boolean;
  hasAttempt: boolean;
  acknowledged: boolean;
  canCheck: boolean;
  error: string;
  message: string;
  change: (field: "entry" | "position" | "label", value: string) => void;
  save: () => Promise<void>;
  retry: () => Promise<void>;
}) {
  function submit(event: FormEvent) {
    event.preventDefault();
    void save();
  }
  const locked = pending || hasAttempt;
  return (
    <form className="playlist-import-form" onSubmit={submit}>
      <label>
        Chapter
        <select
          value={draft.entry}
          disabled={locked}
          onChange={(event) => change("entry", event.target.value)}
          required
        >
          <option value="">Choose a chapter</option>
          {chapters.map((chapter, index) => (
            <option
              key={chapter.entry_id}
              value={chapter.entry_id}
              disabled={!chapter.recording_id || !chapter.available}
            >
              {`Chapter ${index + 1}: ${chapter.title || "Untitled chapter"}`}
            </option>
          ))}
        </select>
      </label>
      <label>
        Position in seconds
        <input
          type="number"
          min="0"
          step="0.001"
          value={draft.position}
          disabled={locked}
          onChange={(event) => change("position", event.target.value)}
          required
        />
      </label>
      <label>
        Bookmark label
        <input
          value={draft.label}
          disabled={locked}
          onChange={(event) => change("label", event.target.value)}
          required
        />
      </label>
      <button
        className="button primary"
        type="submit"
        disabled={
          locked || !draft.entry || !draft.position || !draft.label.trim()
        }
      >
        Save bookmark
      </button>
      {pending && (
        <p role="status">
          {acknowledged || canCheck
            ? "Checking bookmarks…"
            : "Saving bookmark change…"}
        </p>
      )}
      {message && <p role="status">{message}</p>}
      {error && <p role="alert">{error}</p>}
      {(hasAttempt || canCheck) && !pending && error && (
        <button className="button" type="button" onClick={() => void retry()}>
          {acknowledged || canCheck
            ? "Check bookmarks"
            : "Retry bookmark change"}
        </button>
      )}
    </form>
  );
}
