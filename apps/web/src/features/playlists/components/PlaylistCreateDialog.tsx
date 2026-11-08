import type { Schema } from "../../../shared/schema";
import { createUuid } from "../../../shared/uuid";
import { useState, type FormEvent } from "react";
import { router } from "@inertiajs/react";
import { useMutation } from "../../../shared/hooks/useMutation";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import { duplicatePlaylistWarning } from "../model/duplicateName";

export default function PlaylistCreateDialog({
  account,
  playlists,
  onClose,
}: {
  account: string;
  playlists: Schema<"Playlist">[];
  onClose: () => void;
}) {
  const [name, setName] = useState("");
  const { mutate, pending } = useMutation(account);
  const duplicate = duplicatePlaylistWarning(playlists, name);
  async function create(event: FormEvent) {
    event.preventDefault();
    if (!name.trim() || pending) return;
    const result = await mutate({
      kind: "playlist_create",
      playlist_id: createUuid(),
      name,
    });
    if (!result.ok) return;
    router.reload();
    onClose();
  }
  return (
    <Dialog title="New playlist" busy={pending} onClose={onClose}>
      <form className="dialog-form" onSubmit={create}>
        <label>
          Name
          <input
            value={name}
            maxLength={256}
            placeholder="Give it a name"
            autoComplete="off"
            required
            onChange={(event) => setName(event.target.value)}
          />
        </label>
        {duplicate && (
          <p className="name-warning" role="status">
            {duplicate} You can still create another.
          </p>
        )}
        <div className="dialog-actions">
          <Button disabled={pending} onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant="primary"
            type="submit"
            disabled={pending || !name.trim()}
          >
            Create playlist
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
