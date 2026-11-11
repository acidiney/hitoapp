import type { Schema } from "../../../shared/schema";
import { duplicatePlaylistWarning } from "../model/duplicateName";

export function PlaylistNameField({
  name,
  disabled,
  onName,
}: {
  name: string;
  disabled: boolean;
  onName: (name: string) => void;
}) {
  return (
    <label className="playlist-name-field">
      <span>Playlist name</span>
      <input
        value={name}
        maxLength={256}
        disabled={disabled}
        aria-invalid={name.trim() === ""}
        onChange={(event) => onName(event.target.value)}
      />
    </label>
  );
}

export default function PlaylistEditPanel({
  playlistId,
  playlists,
  name,
  dirty,
}: {
  playlistId: string;
  playlists: Schema<"Playlist">[];
  name: string;
  dirty: boolean;
}) {
  const duplicate = duplicatePlaylistWarning(playlists, name, playlistId);
  return (
    <section className="edit-panel" aria-label="Edit playlist">
      {duplicate && (
        <p className="name-warning" role="status">
          {duplicate} You can still use this name.
        </p>
      )}
      <p className="muted">
        Use the arrows to change the order and the cross to remove a track.
        Nothing is saved until you press Done editing.
      </p>
      {dirty && <p className="edit-dirty">You have unsaved changes.</p>}
    </section>
  );
}
