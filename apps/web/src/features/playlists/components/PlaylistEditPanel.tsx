import type { Schema } from "../../../shared/schema";
import { duplicatePlaylistWarning } from "../model/duplicateName";

export default function PlaylistEditPanel({
  playlistId,
  playlists,
  name,
  dirty,
  disabled,
  onName,
}: {
  playlistId: string;
  playlists: Schema<"Playlist">[];
  name: string;
  dirty: boolean;
  disabled: boolean;
  onName: (name: string) => void;
}) {
  const duplicate = duplicatePlaylistWarning(playlists, name, playlistId);
  return (
    <section className="edit-panel" aria-label="Edit playlist">
      <label>
        Name
        <input
          value={name}
          maxLength={256}
          disabled={disabled}
          aria-invalid={name.trim() === ""}
          onChange={(event) => onName(event.target.value)}
        />
      </label>
      {duplicate && (
        <p className="name-warning" role="status">
          {duplicate} You can still use this name.
        </p>
      )}
      <p className="muted">
        Use the arrows to change the order and the cross to remove a track.
        Nothing changes until you press Done. Cancel throws your changes away.
        Changes stay in Hito; a provider playlist is not changed.
      </p>
      {dirty && <p className="edit-dirty">You have unsaved changes.</p>}
    </section>
  );
}
