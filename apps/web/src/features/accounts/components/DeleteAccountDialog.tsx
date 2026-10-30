import { useState, type FormEvent } from "react";
import type { Schema } from "../../../shared/schema";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";

export default function DeleteAccountDialog({
  user,
  pending,
  onDelete,
  onClose,
}: {
  user: Schema<"User">;
  pending: boolean;
  onDelete: (user: Schema<"User">) => Promise<string | null>;
  onClose: () => void;
}) {
  const [typed, setTyped] = useState("");
  const [error, setError] = useState("");
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (pending || typed !== user.username) return;
    const failure = await onDelete(user);
    if (failure === null) onClose();
    else setError(failure);
  }
  return (
    <Dialog title="Delete account" busy={pending} onClose={onClose}>
      <p className="muted">
        This signs {user.username} out everywhere and permanently erases their
        likes, listening history, playlists, personal edits, provider
        connections, downloads and settings. Shared library files stay. This
        cannot be undone.
      </p>
      <form className="dialog-form" onSubmit={submit}>
        <label>
          Type {user.username} to confirm
          <input
            autoComplete="off"
            value={typed}
            onChange={(event) => setTyped(event.target.value)}
          />
        </label>
        {error && (
          <p className="form-error" role="alert">
            {error}
          </p>
        )}
        <div className="dialog-actions">
          <Button disabled={pending} onClick={onClose}>
            Cancel
          </Button>
          <Button
            type="submit"
            variant="destructive"
            disabled={pending || typed !== user.username}
          >
            Delete account
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
