import { useState, type FormEvent } from "react";
import { router } from "@inertiajs/react";
import type { RecordingMetadata } from "../types";
import { useMutation } from "../hooks/useMutation";
import { usePlayer } from "../player/PlaybackProvider";

export default function RecordingTitleEditor({
  account,
  recording,
  title,
}: {
  account: string;
  recording: string;
  title: RecordingMetadata["title"];
}) {
  const [draft, setDraft] = useState(title.value);
  const [error, setError] = useState("");
  const { mutate, pending } = useMutation(account);
  const player = usePlayer();
  async function save(inherit: boolean) {
    setError("");
    const result = await mutate(
      inherit
        ? { kind: "recording_title_inherit", recording_id: recording }
        : {
            kind: "recording_title_set",
            recording_id: recording,
            title: draft,
          },
    );
    if (!result.ok) {
      setError(result.error);
      return;
    }
    player.setRecordingTitle(
      recording,
      inherit ? null : draft,
      title.inherited,
    );
    router.reload();
  }
  function submit(event: FormEvent) {
    event.preventDefault();
    if (!pending) void save(false);
  }
  return (
    <section
      className="metadata-editor"
      aria-labelledby="personal-title-heading"
    >
      <h2 id="personal-title-heading">Your track title</h2>
      <p>
        Changes apply to your account across this recording's albums and
        playlists.
      </p>
      <form onSubmit={submit}>
        <label htmlFor="personal-track-title">Track title</label>
        <input
          id="personal-track-title"
          value={draft}
          required
          disabled={pending}
          onChange={(event) => setDraft(event.target.value)}
        />
        <span className="metadata-inheritance">
          {title.personal === null
            ? "Following source metadata"
            : "Personal title"}{" "}
          · Source title: <strong>{title.inherited}</strong>
        </span>
        {error && (
          <p className="form-error" role="alert">
            {error}
          </p>
        )}
        <div className="metadata-editor-actions">
          {title.personal !== null && (
            <button
              type="button"
              className="button"
              disabled={pending}
              onClick={() => void save(true)}
            >
              Use source title
            </button>
          )}
          <button
            className="button primary"
            disabled={
              pending ||
              !draft.trim() ||
              (title.personal !== null && draft === title.personal)
            }
          >
            {pending ? "Saving…" : "Save personal title"}
          </button>
        </div>
      </form>
    </section>
  );
}
