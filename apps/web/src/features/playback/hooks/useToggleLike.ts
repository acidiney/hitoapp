import { useCallback } from "react";
import { router } from "@inertiajs/react";
import type { Schema } from "../../../shared/schema";
import type { Mutate } from "../../../shared/hooks/useMutation";
import { usePlayer } from "../components/PlaybackProvider";

export function useToggleLike(mutate: Mutate) {
  const player = usePlayer();
  return useCallback(
    async (
      track: { id: string; liked?: boolean },
      onMutationAccepted?: (action: Schema<"Action">) => void,
    ) => {
      const liked = !track.liked;
      const action: Schema<"Action"> = {
        kind: "like_set",
        recording_id: track.id,
        liked,
      };
      if (!(await mutate(action)).ok) return;
      player.setRecordingLiked?.(track.id, liked);
      if (onMutationAccepted) onMutationAccepted(action);
      else router.reload();
    },
    [mutate, player],
  );
}
