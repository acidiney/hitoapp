import { useState } from "react";
import { router } from "@inertiajs/react";
import { useMutation } from "../../../shared/hooks/useMutation";
import type { PlaylistEntry } from "../../../shared/types";
import { reorderMoves } from "../model/reorder";
import { log } from "../../../shared/log";

export function usePlaylistEdit(
  account: string,
  playlist: string,
  entries: PlaylistEntry[],
) {
  const { mutate, pending } = useMutation(account);
  const [editing, setEditing] = useState(false);
  async function move(entryId: string, delta: -1 | 1) {
    const moves = reorderMoves(entries, entryId, delta);
    if (moves.length === 0) return;
    log.info("playlist.reorder", { playlist_id: playlist });
    for (const step of moves) {
      const result = await mutate({
        kind: "entry_move",
        playlist_id: playlist,
        entry_id: step.entry_id,
        position: step.position,
      });
      if (!result.ok) return;
    }
    router.reload();
  }
  async function rename(name: string) {
    const trimmed = name.trim();
    if (!trimmed) return false;
    log.info("playlist.rename", { playlist_id: playlist });
    const result = await mutate({
      kind: "playlist_rename",
      playlist_id: playlist,
      name: trimmed,
    });
    if (result.ok) router.reload();
    return result.ok;
  }
  return { editing, setEditing, move, rename, pending };
}
