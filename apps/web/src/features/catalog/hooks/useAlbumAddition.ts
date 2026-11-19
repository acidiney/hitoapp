import { useCallback, useEffect, useState } from "react";
import type { Schema } from "../../../shared/schema";
import { errorMessage } from "../../../shared/format";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import { albumAdditionApi } from "../api/albumAdditionApi";

export type AlbumAdditionSource = {
  key: string;
  missing: number;
  total: number;
  add: (
    requestId: string,
    signal: AbortSignal,
  ) => Promise<Schema<"AlbumAddition">>;
  refresh: () => Promise<void>;
};

export type AlbumAdditionState = {
  missing: number;
  total: number;
  phase: "add" | "undo" | null;
  added: Schema<"AlbumAddition"> | null;
  undone: Schema<"AlbumAdditionUndone"> | null;
  error: string;
  add: () => Promise<void>;
  undo: () => Promise<void>;
};

export function useAlbumAddition(
  account: string,
  source: AlbumAdditionSource | null,
): AlbumAdditionState | null {
  const key = source?.key ?? "";
  const scope = useRequestScope(`${account}/${key}`);
  const [phase, setPhase] = useState<"add" | "undo" | null>(null);
  const [added, setAdded] = useState<Schema<"AlbumAddition"> | null>(null);
  const [undone, setUndone] = useState<Schema<"AlbumAdditionUndone"> | null>(
    null,
  );
  const [error, setError] = useState("");
  useEffect(() => {
    setAdded(null);
    setUndone(null);
    setError("");
  }, [account, key]);

  const add = useCallback(async () => {
    if (!source) return;
    const captured = scope.capture();
    setPhase("add");
    setError("");
    setUndone(null);
    let outcome = "failed";
    try {
      const result = await source.add(createUuid(), captured.signal);
      if (!captured.active()) return;
      outcome = "accepted";
      setAdded(result);
      await source.refresh();
    } catch (failure) {
      if (captured.active()) setError(errorMessage(failure));
    } finally {
      if (captured.active()) setPhase(null);
      log.info("album_addition.finished", { reason: outcome });
    }
  }, [source, scope]);

  const undo = useCallback(async () => {
    if (!added) return;
    const captured = scope.capture();
    setPhase("undo");
    setError("");
    let outcome = "failed";
    try {
      const result = await albumAdditionApi.undo(
        account,
        added.id,
        captured.signal,
      );
      if (!captured.active()) return;
      outcome = "accepted";
      setUndone(result);
      setAdded(null);
      await source?.refresh();
    } catch (failure) {
      if (captured.active()) setError(errorMessage(failure));
    } finally {
      if (captured.active()) setPhase(null);
      log.info("album_addition.undo_finished", { reason: outcome });
    }
  }, [account, added, source, scope]);

  if (!source && !added && !undone) return null;
  return {
    missing: source?.missing ?? 0,
    total: source?.total ?? 0,
    phase,
    added,
    undone,
    error,
    add,
    undo,
  };
}
