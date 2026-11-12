import { useEffect } from "react";
import { onInvalidated } from "../../../shared/invalidation";
import type { Player } from "../model";

export function useRecordingSync(player: Player) {
  useEffect(
    () =>
      onInvalidated("recording", (id, change) => {
        if (!id || !change) return;
        if ("liked" in change) player.setRecordingLiked?.(id, change.liked);
        else
          player.setRecordingField?.(
            id,
            change.field,
            change.value,
            change.value,
          );
      }),
    [player],
  );
}
