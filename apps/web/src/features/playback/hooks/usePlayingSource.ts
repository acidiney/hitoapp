import { useEffect, useRef, useSyncExternalStore } from "react";
import { sourceStatusStore } from "../../sources/model/sourceStatusStore";
import { usePlayer } from "../components/PlaybackProvider";

export function usePlayingSource(account: string, recording: string | null) {
  const player = usePlayer();
  const status = useSyncExternalStore(sourceStatusStore.subscribe, () =>
    recording ? sourceStatusStore.peek(account, recording) : null,
  );
  const settledPlayback = useSyncExternalStore(player.subscribe, () => {
    const state = player.snapshot();
    return `${state.current?.id ?? ""}|${state.loading}|${state.playing}`;
  });
  const lastPlayback = useRef<string | null>(null);
  useEffect(() => {
    if (!recording) return;
    sourceStatusStore.refresh(account, recording);
  }, [account, recording]);
  useEffect(() => {
    const previous = lastPlayback.current;
    lastPlayback.current = settledPlayback;
    if (!recording || previous === null || previous === settledPlayback) return;
    const known = sourceStatusStore.peek(account, recording);
    if (!known.source?.ready) sourceStatusStore.invalidate(account, recording);
  }, [account, recording, settledPlayback]);
  if (!recording || !status) return { loading: false, source: null };
  return status;
}
