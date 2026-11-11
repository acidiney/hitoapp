import { useSyncExternalStore } from "react";
import { usePlayer } from "../../playback/components/PlaybackProvider";
import { SyncedLines } from "../../playback/components/LyricLines";
import type { TimedLine } from "../model/lyricContent";

export default function FollowedLyrics({
  recording,
  lines,
  fallback,
}: {
  recording: string;
  lines: TimedLine[];
  fallback: React.ReactNode;
}) {
  const player = usePlayer();
  const playing = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().current?.id === recording,
  );
  return playing ? <SyncedLines lines={lines} /> : <>{fallback}</>;
}
