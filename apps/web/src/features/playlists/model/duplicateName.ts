import type { Schema } from "../../../shared/schema";

export function duplicatePlaylistWarning(
  playlists: Pick<Schema<"Playlist">, "id" | "name">[],
  name: string,
  ownId?: string,
): string | null {
  const typed = name.trim();
  if (!typed) return null;
  const folded = typed.toLocaleLowerCase();
  const taken = playlists.some(
    (playlist) =>
      playlist.id !== ownId &&
      playlist.name.trim().toLocaleLowerCase() === folded,
  );
  return taken ? `You already have a playlist called “${typed}”.` : null;
}
