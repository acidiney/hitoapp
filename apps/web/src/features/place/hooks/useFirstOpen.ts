import { useCallback, useState } from "react";

export type FirstOpenPhase = "choosing" | "starting" | "failed";

export function useFirstOpen(startHere: () => Promise<void>) {
  const [phase, setPhase] = useState<FirstOpenPhase>("choosing");
  const start = useCallback(() => {
    setPhase("starting");
    startHere().catch(() => setPhase("failed"));
  }, [startHere]);
  return { phase, start };
}
