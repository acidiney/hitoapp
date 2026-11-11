import { Heart } from "lucide-react";
import type { Track } from "../model";
import { useToggleLike } from "../hooks/useToggleLike";
import { useTrackActions } from "./TrackActionsProvider";

export default function NowPlayingLike({ track }: { track: Track }) {
  const actions = useTrackActions();
  const toggle = useToggleLike(
    actions?.mutate ?? (async () => ({ ok: false }) as never),
  );
  if (!actions) return null;
  const liked = track.liked ?? false;
  return (
    <button
      type="button"
      className="np-like"
      aria-label={`${liked ? "Unlike" : "Like"} ${track.title}`}
      aria-pressed={liked}
      onClick={() => void toggle({ id: track.id, liked })}
    >
      <Heart
        size={22}
        fill={liked ? "currentColor" : "none"}
        aria-hidden="true"
      />
    </button>
  );
}
