import { useSyncExternalStore } from "react";
import type { Track } from "../model";
import StatusDot from "../../../shared/ui/StatusDot";
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
  if (!track.plays_from_services) return <StatusDot status="none" as={Tag} />;
  if (current === track.id) return null;
  return <StatusDot status="ready" detail="from your services" as={Tag} />;
}
