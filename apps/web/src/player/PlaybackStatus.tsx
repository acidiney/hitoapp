import { useSyncExternalStore } from "react";
import { usePlayer } from "./PlaybackProvider";

export default function PlaybackStatus({
  className = "playback-status",
}: {
  className?: string;
}) {
  const player = usePlayer();
  const loading = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().loading,
  );
  const error = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().error,
  );
  if (!loading && !error) return null;
  return (
    <div className={className} role="status">
      {loading ? "Finding audio…" : error}
    </div>
  );
}
