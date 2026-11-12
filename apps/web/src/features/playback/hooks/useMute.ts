import { useCallback } from "react";
import { usePlayer } from "../components/PlaybackProvider";

let volumeBeforeMute = 0.7;

export function useMute() {
  const player = usePlayer();
  return useCallback(() => {
    const volume = player.snapshot().volume;
    if (volume > 0) volumeBeforeMute = volume;
    player.volume(volume > 0 ? 0 : volumeBeforeMute);
  }, [player]);
}
