import { useCallback, useEffect, useRef, useState } from "react";
import type { ConnectOutcome, ServerPlace } from "../api/desktopPlace";
import type { ConnectProblem } from "./useConnectServer";

export const RETRY_MS = 10_000;

export type SavedPhase =
  | { kind: "opening" }
  | { kind: "unreachable"; shown: string }
  | { kind: "certificate"; host: string }
  | { kind: "other"; outcome: ConnectProblem };

function phaseFor(outcome: ConnectOutcome): SavedPhase {
  switch (outcome.outcome) {
    case "opened":
    case "busy":
      return { kind: "opening" };
    case "no_answer":
      return { kind: "unreachable", shown: outcome.shown };
    case "certificate":
      return { kind: "certificate", host: outcome.host };
    case "not_encrypted":
      return { kind: "other", outcome: { outcome: "failed" } };
    default:
      return { kind: "other", outcome };
  }
}

export function useSavedServer(place: ServerPlace, origin: string) {
  const [phase, setPhase] = useState<SavedPhase>({ kind: "opening" });
  const live = useRef(true);
  const tryNow = useCallback(async () => {
    let outcome: ConnectOutcome;
    try {
      outcome = await place.connect(origin, false);
    } catch {
      outcome = { outcome: "failed" };
    }
    if (live.current) setPhase(phaseFor(outcome));
  }, [place, origin]);

  useEffect(() => {
    live.current = true;
    void tryNow();
    return () => {
      live.current = false;
    };
  }, [tryNow]);

  useEffect(() => {
    if (phase.kind !== "unreachable") return;
    const timer = window.setInterval(() => void tryNow(), RETRY_MS);
    return () => window.clearInterval(timer);
  }, [phase.kind, tryNow]);

  return { phase, tryNow };
}
