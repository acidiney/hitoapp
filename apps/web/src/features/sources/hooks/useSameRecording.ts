import { useCallback, useEffect, useRef, useState } from "react";
import type { Schema } from "../../../shared/schema";
import { errorMessage } from "../../../shared/format";
import { createUuid } from "../../../shared/uuid";
import { communityApi } from "../../community/api/communityApi";
import { sameRecordingApi } from "../api/sameRecordingApi";

type Source = Schema<"Source">;

export type SameRecordingRow =
  | { kind: "sending" }
  | { kind: "sent" }
  | { kind: "failed"; error: string; other: string };

export function useSameRecording(account: string, sources: Source[]) {
  const candidates = sources.filter((source) => source.community_public);
  const offered = candidates.length >= 2;
  const [consent, setConsent] = useState<boolean | null>(null);
  const [picking, setPicking] = useState<Source | null>(null);
  const [rows, setRows] = useState<Record<string, SameRecordingRow>>({});
  const inFlight = useRef<AbortController | null>(null);

  useEffect(() => {
    if (!offered) return;
    const controller = new AbortController();
    communityApi
      .preferences(account, controller.signal)
      .then((preferences) => setConsent(preferences.metadata_corrections))
      .catch(() => {
        if (!controller.signal.aborted) setConsent(false);
      });
    return () => controller.abort();
  }, [account, offered]);

  useEffect(() => () => inFlight.current?.abort(), []);

  const send = useCallback(
    async (from: string, other: string) => {
      inFlight.current?.abort();
      const controller = new AbortController();
      inFlight.current = controller;
      setPicking(null);
      setRows((current) => ({ ...current, [from]: { kind: "sending" } }));
      try {
        await sameRecordingApi.suggest(
          account,
          from,
          { submission_id: createUuid(), other_source_id: other },
          controller.signal,
        );
        if (!controller.signal.aborted)
          setRows((current) => ({ ...current, [from]: { kind: "sent" } }));
      } catch (error) {
        if (controller.signal.aborted) return;
        setRows((current) => ({
          ...current,
          [from]: { kind: "failed", error: errorMessage(error), other },
        }));
      }
    },
    [account],
  );

  const cancel = useCallback(() => {
    inFlight.current?.abort();
    setPicking(null);
    setRows((current) =>
      Object.fromEntries(
        Object.entries(current).filter(([, row]) => row.kind !== "sending"),
      ),
    );
  }, []);

  return {
    offered,
    candidates,
    consent,
    picking,
    rows,
    pick: setPicking,
    send,
    cancel,
  };
}
