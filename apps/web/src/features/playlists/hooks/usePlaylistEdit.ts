import { useState } from "react";
import { router } from "@inertiajs/react";
import { useMutation } from "../../../shared/hooks/useMutation";
import { useNotice } from "../../../shared/ui/Notices";
import type { PlaylistEntry } from "../../../shared/types";
import { reorderMoves } from "../model/reorder";
import { log } from "../../../shared/log";

export function usePlaylistEdit(
  account: string,
  playlist: string,
  entries: PlaylistEntry[],
) {
  const { mutate, mutateMany, pending } = useMutation(account);
  const notice = useNotice();
  const [editing, setEditing] = useState(false);
  const [reloading, setReloading] = useState(false);
  function reload() {
    setReloading(true);
    router.reload({ onFinish: () => setReloading(false) });
  }
  async function move(entryId: string, delta: -1 | 1) {
    if (pending || reloading) return;
    const moves = reorderMoves(entries, entryId, delta);
    if (moves === null) {
      notice("This playlist's order cannot be changed here. Try again later.");
      return;
    }
    if (moves.length === 0) return;
    log.info("playlist.reorder", { playlist_id: playlist });
    await mutateMany(
      moves.map((step) => ({
        kind: "entry_move" as const,
        playlist_id: playlist,
        entry_id: step.entry_id,
        position: step.position,
      })),
    );
    reload();
  }
  async function rename(name: string) {
    const trimmed = name.trim();
    if (!trimmed || pending || reloading) return false;
    log.info("playlist.rename", { playlist_id: playlist });
    const result = await mutate({
      kind: "playlist_rename",
      playlist_id: playlist,
      name: trimmed,
    });
    if (result.ok) reload();
    return result.ok;
  }
  return { editing, setEditing, move, rename, pending: pending || reloading };
}
