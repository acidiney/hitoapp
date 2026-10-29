import { useSyncExternalStore } from "react";
import { usePlayer } from "./PlaybackProvider";

export const SPEED_CHOICES = [0.5, 0.75, 1, 1.25, 1.5, 2];

export default function SpeedControl({ available }: { available: boolean }) {
  const player = usePlayer();
  const speed = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().speed,
  );
  const range = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().speedRange ?? null,
  );
  return (
    <select
      aria-label="Playback speed"
      value={speed}
      disabled={!player.supports.speed || !available}
      title={
        !player.supports.speed
          ? "Playback speed is not available in the desktop player yet"
          : undefined
      }
      onChange={(event) => player.speed(+event.target.value)}
    >
      {SPEED_CHOICES.map((choice) => (
        <option
          key={choice}
          value={choice}
          disabled={
            range !== null &&
            choice !== speed &&
            (choice < range.min || choice > range.max)
          }
        >
          {choice}×
        </option>
      ))}
    </select>
  );
}
