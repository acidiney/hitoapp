import type { Schema } from "../../../shared/schema";
import { useMemo, useState } from "react";
import { router } from "@inertiajs/react";
import { useMutation } from "../../../shared/hooks/useMutation";
import { useNotice } from "../../../shared/ui/Notices";

import { applyMoveSteps, type MoveStep } from "../model/reorder";
import { log } from "../../../shared/log";

const MAX_CHANGES = 100;

type Draft = { name: string; steps: MoveStep[]; removed: ReadonlySet<string> };

export function usePlaylistEdit(
  account: string,
  playlist: string,
  savedName: string,
  entries: Schema<"PlaylistEntry">[],
) {
  const { mutateMany, pending } = useMutation(account);
  const notice = useNotice();
  const [editing, setEditing] = useState(false);
  const [reloading, setReloading] = useState(false);
  const [draft, setDraft] = useState<Draft>({
    name: savedName,
    steps: [],
    removed: new Set(),
  });
  const ordered = useMemo(
    () => applyMoveSteps(entries, draft.steps) ?? { entries, moves: [] },
    [entries, draft.steps],
  );
  const name = draft.name.trim();
  const renamed = name !== "" && name !== savedName;
  const dirty =
    renamed ||
    draft.name !== savedName ||
    draft.steps.length > 0 ||
    draft.removed.size > 0;
  const busy = pending || reloading;

  function begin() {
    setDraft({ name: savedName, steps: [], removed: new Set() });
    setEditing(true);
  }
  function cancel() {
    setDraft({ name: savedName, steps: [], removed: new Set() });
    setEditing(false);
  }
  function setName(next: string) {
    setDraft((current) => ({ ...current, name: next }));
  }
  function move(entryId: string, delta: -1 | 1) {
    if (busy) return;
    const steps = [...draft.steps, { entry_id: entryId, delta }];
    if (applyMoveSteps(entries, steps) === null) {
      notice("This playlist's order cannot be changed here. Try again later.");
      return;
    }
    setDraft((current) => ({ ...current, steps }));
  }
  function toggleRemove(entryId: string) {
    setDraft((current) => {
      const removed = new Set(current.removed);
      if (!removed.delete(entryId)) removed.add(entryId);
      return { ...current, removed };
    });
  }
  async function done() {
    if (busy) return;
    if (!dirty) return setEditing(false);
    if (name === "") return;
    const actions: Schema<"Action">[] = [
      ...(renamed
        ? [
            {
              kind: "playlist_rename" as const,
              playlist_id: playlist,
              name,
            },
          ]
        : []),
      ...[...draft.removed].map((entryId) => ({
        kind: "entry_remove" as const,
        playlist_id: playlist,
        entry_id: entryId,
      })),
      ...ordered.moves
        .filter((step) => !draft.removed.has(step.entry_id))
        .map((step) => ({
          kind: "entry_move" as const,
          playlist_id: playlist,
          entry_id: step.entry_id,
          position: step.position,
        })),
    ];
    if (actions.length > MAX_CHANGES) {
      notice(
        "That is too many changes to save at once. Cancel and make them in smaller steps.",
      );
      return;
    }
    log.info("playlist.edit_saved", {
      playlist_id: playlist,
      count: actions.length,
    });
    const result = await mutateMany(actions);
    if (!result.ok) return;
    setReloading(true);
    router.reload({
      onFinish: () => {
        setReloading(false);
        setDraft({ name: savedName, steps: [], removed: new Set() });
        setEditing(false);
      },
    });
  }
  return {
    editing,
    begin,
    cancel,
    done,
    dirty,
    name: draft.name,
    setName,
    entries: ordered.entries,
    removed: draft.removed,
    move,
    toggleRemove,
    pending: busy,
  };
}
