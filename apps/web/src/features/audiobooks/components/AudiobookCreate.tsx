import type { FormEvent } from "react";
import type { Playlist } from "../../../shared/types";

export default function AudiobookCreate({
  playlists,
  selected,
  pending,
  hasAttempt,
  acknowledged,
  canChooseAnother,
  error,
  select,
  add,
  retry,
  chooseAnother,
}: {
  playlists: Playlist[];
  selected: string;
  pending: boolean;
  hasAttempt: boolean;
  acknowledged: boolean;
  canChooseAnother: boolean;
  error: string;
  select: (id: string) => void;
  add: () => Promise<void>;
  retry: () => Promise<void>;
  chooseAnother: () => void;
}) {
  function submit(event: FormEvent) {
    event.preventDefault();
    void add();
  }
  return (
    <section className="playlist-import" aria-label="Add an audiobook">
      <h2>Add an audiobook</h2>
      <p className="muted">
        Use an editable playlist's entries as ordered chapters.
      </p>
      <form className="playlist-import-form" onSubmit={submit}>
        <label>
          Playlist
          <select
            value={selected}
            disabled={pending || hasAttempt || playlists.length === 0}
            onChange={(event) => select(event.target.value)}
            required
          >
            <option value="">Choose a playlist</option>
            {playlists.map((playlist) => (
              <option key={playlist.id} value={playlist.id}>
                {playlist.name}
              </option>
            ))}
          </select>
        </label>
        <button
          className="button primary"
          type="submit"
          disabled={pending || hasAttempt || !selected}
        >
          Add audiobook
        </button>
        {pending && (
          <p role="status">
            {acknowledged ? "Checking audiobook…" : "Adding audiobook…"}
          </p>
        )}
        {error && <p role="alert">{error}</p>}
        {hasAttempt && !pending && error && (
          <button className="button" type="button" onClick={() => void retry()}>
            {acknowledged ? "Check audiobook" : "Retry adding audiobook"}
          </button>
        )}
        {canChooseAnother && (
          <button className="button" type="button" onClick={chooseAnother}>
            Choose another playlist
          </button>
        )}
      </form>
    </section>
  );
}
