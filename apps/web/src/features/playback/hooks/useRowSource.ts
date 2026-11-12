import { useEffect, useSyncExternalStore } from "react";
import type { Schema } from "../../../shared/schema";
import { onInvalidated } from "../../../shared/invalidation";
import { sourceStatusStore } from "../../sources/model/sourceStatusStore";

export function useRowSource(
  account: string | null,
  recording: string,
  settled: boolean,
): Schema<"Source"> | null {
  useEffect(() => {
    if (!account || settled) return;
    const refresh = (id: string | null) => {
      if (id === recording) sourceStatusStore.refresh(account, recording);
    };
    const stopResolved = onInvalidated("resolved", refresh);
    const stopSources = onInvalidated("sources", refresh);
    return () => {
      stopResolved();
      stopSources();
    };
  }, [account, recording, settled]);
  return useSyncExternalStore(sourceStatusStore.subscribe, () =>
    account ? (sourceStatusStore.peekSettled(account, recording)?.source ?? null) : null,
  );
}
