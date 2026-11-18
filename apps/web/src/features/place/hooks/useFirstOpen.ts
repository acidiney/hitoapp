import { useCallback, useState } from "react";

export type FirstOpenPhase = "choosing" | "starting" | "failed";

export function useFirstOpen(useThisComputer: () => Promise<void>) {
  const [phase, setPhase] = useState<FirstOpenPhase>("choosing");
  const start = useCallback(() => {
    setPhase("starting");
    useThisComputer().catch(() => setPhase("failed"));
  }, [useThisComputer]);
  return { phase, start };
}
