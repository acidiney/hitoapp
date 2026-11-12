import { useMemo, useSyncExternalStore } from "react";
import { usePlayer } from "../components/PlaybackProvider";
import type { Player } from "../model";

export type NowPlaying = {
  recording: string | null;
  entry: string | null;
  playing: boolean;
};

export type RowPlayState = "playing" | "paused" | null;

type RowIdentity = { id: string; entry_id?: string | null };

const SEPARATOR = "\u0000";

function entryOf(current: object | null): string {
  const entry = (current as { entry_id?: unknown } | null)?.entry_id;
  return typeof entry === "string" ? entry : "";
}

function nowPlayingKey(player: Player): string {
  const { current, playing } = player.snapshot();
  if (!current) return "";
  return [current.id, entryOf(current), playing ? "1" : "0"].join(SEPARATOR);
}

export function parseNowPlaying(key: string): NowPlaying {
  if (!key) return { recording: null, entry: null, playing: false };
  const [recording, entry, playing] = key.split(SEPARATOR);
  return { recording, entry: entry || null, playing: playing === "1" };
}

export function rowPlayState(
  now: NowPlaying,
  row: RowIdentity | null,
): RowPlayState {
  if (!row || now.recording === null || now.recording !== row.id) return null;
  if (now.entry !== null && row.entry_id && now.entry !== row.entry_id)
    return null;
  return now.playing ? "playing" : "paused";
}

export function useNowPlaying(): NowPlaying {
  const player = usePlayer();
  const key = useSyncExternalStore(player.subscribe, () =>
    nowPlayingKey(player),
  );
  return useMemo(() => parseNowPlaying(key), [key]);
}
