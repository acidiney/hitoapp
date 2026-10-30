import type { Schema } from "../../../shared/schema";
import { useState, type FormEvent } from "react";
import { Plus } from "lucide-react";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";

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
  playlists: Schema<"Playlist">[];
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
  const [open, setOpen] = useState(false);
  function submit(event: FormEvent) {
    event.preventDefault();
    void add();
  }
  return (
    <div className="media-index-action">
      <Button
        variant="action"
        aria-haspopup="dialog"
        aria-label="Add audiobook from a playlist"
        onClick={() => setOpen(true)}
      >
        <Plus size={17} aria-hidden="true" />
        Add audiobook
      </Button>
      {!open && hasAttempt && !pending && (
        <div className="media-index-pending">
          <p role="status">Your audiobook request was not confirmed.</p>
          <Button onClick={() => setOpen(true)}>
            Review audiobook request
          </Button>
        </div>
      )}
      {open && (
        <Dialog
          title="Add an audiobook"
          busy={pending}
          onClose={() => setOpen(false)}
        >
          <p className="muted">
            Choose an editable playlist. Its entries, in order, become the
            chapters of the audiobook.
          </p>
          <form className="dialog-form" onSubmit={submit}>
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
            {playlists.length === 0 && (
              <p className="muted">
                No editable playlist is available to group. Create a playlist
                first.
              </p>
            )}
            {pending && (
              <p role="status">
                {acknowledged ? "Checking audiobook…" : "Adding audiobook…"}
              </p>
            )}
            {error && <p role="alert">{error}</p>}
            <div className="dialog-actions">
              {hasAttempt && !pending && error && (
                <Button onClick={() => void retry()}>
                  {acknowledged ? "Check audiobook" : "Retry adding audiobook"}
                </Button>
              )}
              {canChooseAnother && (
                <Button onClick={chooseAnother}>Choose another playlist</Button>
              )}
              <Button disabled={pending} onClick={() => setOpen(false)}>
                Cancel
              </Button>
              <Button
                variant="primary"
                type="submit"
                disabled={pending || hasAttempt || !selected}
              >
                Add audiobook
              </Button>
            </div>
          </form>
        </Dialog>
      )}
    </div>
  );
}
