import { useEffect, useState } from "react";
import type { Player, QueueView } from "../model";
import { queuedByYouCount, wasQueuedByYou } from "../queuedByYou";

export function useQueuedFirst(player: Player, view: QueueView): number {
  const [count, setCount] = useState(0);
  useEffect(() => {
    const after = (view.currentIndex ?? -1) + 1;
    const room = Math.min(queuedByYouCount(), view.total - after);
    if (room <= 0) {
      setCount(0);
      return;
    }
    const controller = new AbortController();
    player
      .queuePage(after, room, controller.signal)
      .then((page) => {
        if (controller.signal.aborted) return;
        let leading = 0;
        while (
          leading < page.entries.length &&
          wasQueuedByYou(page.entries[leading].id)
        )
          leading++;
        setCount(leading);
      })
      .catch(() => {});
    return () => controller.abort();
  }, [player, view.version, view.currentIndex, view.total]);
  return count;
}
