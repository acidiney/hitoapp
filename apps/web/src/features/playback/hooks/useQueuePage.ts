import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import type { Player, QueuePage, QueueView } from "../model";

type Result = {
  player: Player;
  view: QueueView;
  offset: number;
  attempt: number;
  page?: QueuePage;
  error?: string;
};

const PREVIEW_ENTRIES = 4;

export function useQueuePage(player: Player) {
  const view = useSyncExternalStore(player.subscribe, player.queueSnapshot);
  const limit = PREVIEW_ENTRIES;
  const [attempt, setAttempt] = useState(0);
  const offset = Math.min(view.total, (view.currentIndex ?? -1) + 1);
  const latest = useRef({ player, view, offset, attempt });
  latest.current = { player, view, offset, attempt };
  const [result, setResult] = useState<Result>();
  useEffect(() => {
    if (!view.total) return;
    const controller = new AbortController();
    const active = () =>
      !controller.signal.aborted &&
      latest.current.player === player &&
      latest.current.view === view &&
      latest.current.offset === offset &&
      latest.current.attempt === attempt;
    void player
      .queuePage(offset, limit, controller.signal)
      .then((page) => {
        if (active() && page.version === view.version)
          setResult({ player, view, offset, attempt, page });
      })
      .catch(() => {
        if (active())
          setResult({
            player,
            view,
            offset,
            attempt,
            error: "The queue could not be loaded. Try again.",
          });
      });
    return () => controller.abort();
  }, [player, view, offset, limit, attempt]);
  const current =
    result?.player === player &&
    result.view === view &&
    result.offset === offset &&
    result.attempt === attempt
      ? result
      : undefined;
  return {
    view,
    page: current?.page,
    error: current?.error,
    loading: view.total > 0 && !current,
    retry: () => setAttempt((value) => value + 1),
  };
}
