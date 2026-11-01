import { useSyncExternalStore } from "react";
import type { Track } from "../model";
import { usePlayer } from "./PlaybackProvider";

export default function TrackAvailabilityNote({
  track,
  as: Tag = "small",
}: {
  track: Pick<Track, "id" | "available" | "plays_from_services">;
  as?: "small" | "span";
}) {
  const player = usePlayer();
  const current = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().current?.id,
  );
  if (track.available || track.plays_from_services === undefined) return null;
  if (!track.plays_from_services)
    return (
      <Tag className="availability" data-state="catalog">
        <span className="ring" data-ring="catalog" aria-hidden="true" />
        Not playable yet
      </Tag>
    );
  if (current === track.id) return null;
  return (
    <Tag className="availability" data-state="available">
      <span className="ring" data-ring="available" aria-hidden="true" />
      Plays from your services
    </Tag>
  );
}
