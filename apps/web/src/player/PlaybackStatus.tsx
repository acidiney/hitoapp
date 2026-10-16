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
  const connection = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().connection,
  );
  const resumeFailed = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().resumeFailed,
  );
  if (!loading && !error && connection !== "connecting") return null;
  return (
    <div className={className} role="status">
      <span>
        {connection === "connecting"
          ? "Connecting player…"
          : loading
            ? "Finding audio…"
            : error}
      </span>
      {connection === "ready" && resumeFailed && player.retrySaved && (
        <button
          className="text-link"
          onClick={() => void player.retrySaved?.()}
        >
          Retry saved position
        </button>
      )}
      {connection === "disconnected" && player.reconnect && (
        <button className="text-link" onClick={() => void player.reconnect?.()}>
          Reconnect player
        </button>
      )}
    </div>
  );
}
