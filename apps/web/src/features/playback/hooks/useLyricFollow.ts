import {
  useCallback,
  useEffect,
  useState,
  useSyncExternalStore,
  type RefObject,
} from "react";
import { usePlayer } from "../components/PlaybackProvider";
import { lineAt } from "../../lyrics/model/lineTiming";

type Timing = { start_ms: number; end_ms: number };

function reducedMotion(): boolean {
  return (
    window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false
  );
}

export function useLyricFollow(
  lines: Timing[],
  scroller: RefObject<HTMLElement | null>,
) {
  const player = usePlayer();
  const current = useSyncExternalStore(player.subscribePosition, () =>
    lineAt(lines, player.positionSnapshot().position * 1000),
  );
  const [detached, setDetached] = useState(false);
  const align = useCallback(
    (index: number) => {
      const root = scroller.current;
      const line = root?.querySelector<HTMLElement>(`[data-line="${index}"]`);
      if (!root || !line) return;
      line.scrollIntoView({
        block: "center",
        behavior: reducedMotion() ? "auto" : "smooth",
      });
    },
    [scroller],
  );
  useEffect(() => {
    if (!detached && current >= 0) align(current);
  }, [current, detached, align]);
  const detach = useCallback(() => setDetached(true), []);
  const rejoin = useCallback(() => {
    setDetached(false);
    if (current >= 0) align(current);
  }, [current, align]);
  return { current, detached, detach, rejoin };
}
