import { useCallback, useEffect, useRef, useState } from "react";
import type {
  ConnectOutcome,
  RecentServer,
  ServerPlace,
} from "../api/desktopPlace";

export type ConnectProblem = Exclude<
  ConnectOutcome,
  { outcome: "opened" } | { outcome: "not_encrypted" } | { outcome: "busy" }
>;

export type ConnectPhase =
  | { kind: "editing"; problem: ConnectProblem | null }
  | { kind: "looking" }
  | { kind: "not_encrypted"; origin: string; shown: string }
  | { kind: "opening" };

export function useConnectServer(place: ServerPlace) {
  const [address, setAddress] = useState("");
  const [phase, setPhase] = useState<ConnectPhase>({
    kind: "editing",
    problem: null,
  });
  const [recent, setRecent] = useState<RecentServer[]>([]);
  const attempt = useRef(0);

  useEffect(() => {
    let current = true;
    place
      .recent()
      .then((servers) => current && setRecent(servers))
      .catch(() => {});
    return () => {
      current = false;
    };
  }, [place]);

  const run = useCallback(
    async (typed: string, acceptPlain: boolean) => {
      const mine = ++attempt.current;
      setPhase({ kind: acceptPlain ? "opening" : "looking" });
      let outcome: ConnectOutcome;
      try {
        outcome = await place.connect(typed, acceptPlain);
      } catch {
        outcome = { outcome: "failed" };
      }
      if (mine !== attempt.current) return;
      switch (outcome.outcome) {
        case "opened":
          setPhase({ kind: "opening" });
          return;
        case "not_encrypted":
          setPhase({
            kind: "not_encrypted",
            origin: outcome.origin,
            shown: outcome.shown,
          });
          return;
        case "busy":
          setPhase({ kind: "editing", problem: null });
          return;
        default:
          setPhase({ kind: "editing", problem: outcome });
      }
    },
    [place],
  );

  const connect = useCallback(
    (typed: string) => {
      setAddress(typed);
      void run(typed, false);
    },
    [run],
  );

  const cancel = useCallback(() => {
    attempt.current += 1;
    void place.cancel().catch(() => {});
    setPhase({ kind: "editing", problem: null });
  }, [place]);

  const accept = useCallback(() => {
    if (phase.kind === "not_encrypted") void run(phase.origin, true);
  }, [phase, run]);

  const changeAddress = useCallback(
    () => setPhase({ kind: "editing", problem: null }),
    [],
  );

  const forget = useCallback(
    async (origin: string) => {
      await place.forget(origin);
      setRecent((servers) =>
        servers.filter((server) => server.origin !== origin),
      );
    },
    [place],
  );

  return {
    address,
    setAddress,
    phase,
    recent,
    connect,
    cancel,
    accept,
    changeAddress,
    forget,
  };
}
