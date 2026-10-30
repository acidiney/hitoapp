import { useSyncExternalStore } from "react";
import { usePlayer } from "./PlaybackProvider";

/** Tracks that are not stored here are resolved from the services when played. */
export default function TrackAvailabilityNote({
  trackId,
  as: Tag = "small",
}: {
  trackId: string;
  as?: "small" | "span";
}) {
  const player = usePlayer();
  const current = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().current?.id,
  );
  if (current === trackId) return null;
  return (
    <Tag className="availability" data-state="available">
      <span className="ring" data-ring="available" aria-hidden="true" />
      Plays from your services
    </Tag>
  );
}
