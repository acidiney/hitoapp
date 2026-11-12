import { useMemo, useSyncExternalStore } from "react";
import { sourceStatusStore } from "../../sources/model/sourceStatusStore";

type Entry = { recording_id?: string | null; available: boolean };

export function usePlaylistReadiness(
  account: string | null,
  entries: readonly Entry[],
) {
  const readyKey = useSyncExternalStore(sourceStatusStore.subscribe, () =>
    entries
      .flatMap((entry) => {
        const id = entry.recording_id;
        if (!id) return [];
        if (entry.available) return [id];
        const resolved =
          account === null
            ? null
            : sourceStatusStore.peekSettled(account, id)?.source;
        return resolved?.ready ? [id] : [];
      })
      .join("\u0000"),
  );
  return useMemo(() => {
    const ready = new Set(readyKey === "" ? [] : readyKey.split("\u0000"));
    return {
      count: entries.filter(
        (entry) => entry.recording_id != null && ready.has(entry.recording_id),
      ).length,
      isReady: (recording: string) => ready.has(recording),
    };
  }, [readyKey, entries]);
}
