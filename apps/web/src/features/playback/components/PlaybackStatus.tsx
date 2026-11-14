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
  const retrying = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().retrying ?? null,
  );
  const resumeFailed = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().resumeFailed,
  );
  const speed = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().speed,
  );
  const known = useSyncExternalStore(
    player.subscribe,
    () => !!player.snapshot().speedRange,
  );
  const effectiveSpeed = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().effectiveSpeed ?? player.snapshot().speed,
  );
  if (
    !loading &&
    !retrying &&
    !error &&
    connection === "ready" &&
    known &&
    effectiveSpeed !== speed
  )
    return (
      <div className={`${className} speed-notice`} role="status">
        <span>
          {`This source plays at ${effectiveSpeed}×. Your ${speed}× speed returns with a source that supports it.`}
        </span>
      </div>
    );
  if (!loading && !retrying && !error && connection !== "connecting")
    return null;
  return (
    <div className={className} role="status">
      <span>
        {connection === "connecting"
          ? "Connecting player…"
          : retrying
            ? `Trying again (${retrying.attempt} of ${retrying.of})…`
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
