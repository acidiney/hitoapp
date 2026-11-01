import {
  useCallback,
  useEffect,
  useReducer,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import type { Player, PlaybackOccurrence } from "../model";

export const QUEUE_ROW = 56;
const BLOCK = 50;
const OVERSCAN = 6;
const FALLBACK_HEIGHT = 560;

type Block = { version: number; entries: PlaybackOccurrence[] };
type Viewport = { first: number; height: number };

export function useQueueWindow(player: Player, from = 0, to?: number) {
  const view = useSyncExternalStore(player.subscribe, player.queueSnapshot);
  const [viewport, setViewport] = useState<Viewport>(() => ({
    first: from === 0 ? Math.max(0, (view.currentIndex ?? 0) - 1) : 0,
    height: FALLBACK_HEIGHT,
  }));
  const [attempt, setAttempt] = useState(0);
  const [error, setError] = useState(false);
  const [, rerender] = useReducer((count: number) => count + 1, 0);
  const blocks = useRef({ player, items: new Map<number, Block>() });
  if (blocks.current.player !== player)
    blocks.current = { player, items: new Map() };

  const first = from + viewport.first;
  const start = Math.max(from, first - OVERSCAN);
  const end = Math.min(
    to ?? view.total,
    first + Math.ceil(viewport.height / QUEUE_ROW) + OVERSCAN,
  );
  const firstBlock = Math.floor(start / BLOCK);
  const lastBlock =
    end > start ? Math.floor((end - 1) / BLOCK) : firstBlock - 1;

  useEffect(() => {
    const cache = blocks.current.items;
    for (const index of [...cache.keys()])
      if (index < firstBlock - 1 || index > lastBlock + 1) cache.delete(index);
    const controller = new AbortController();
    for (let block = firstBlock; block <= lastBlock; block++) {
      if (cache.get(block)?.version === view.version) continue;
      player
        .queuePage(block * BLOCK, BLOCK, controller.signal)
        .then((page) => {
          if (controller.signal.aborted || page.version !== view.version)
            return;
          cache.set(block, { version: page.version, entries: page.entries });
          setError(false);
          rerender();
        })
        .catch(() => {
          if (!controller.signal.aborted) setError(true);
        });
    }
    return () => controller.abort();
  }, [player, view.version, firstBlock, lastBlock, attempt]);

  const entryAt = useCallback(
    (index: number) =>
      blocks.current.items.get(Math.floor(index / BLOCK))?.entries[
        index % BLOCK
      ],
    [],
  );
  const idAt = useCallback(
    async (index: number) => {
      const cached = entryAt(index);
      if (cached) return cached.id;
      return (await player.queuePage(index, 1)).entries[0]?.id ?? null;
    },
    [entryAt, player],
  );
  const scrolled = useCallback((top: number, height: number) => {
    const first = Math.max(0, Math.floor(top / QUEUE_ROW));
    const next = height || FALLBACK_HEIGHT;
    setViewport((old) =>
      old.first === first && old.height === next
        ? old
        : { first, height: next },
    );
  }, []);

  return {
    view,
    from,
    start,
    end,
    error,
    entryAt,
    idAt,
    scrolled,
    retry: () => setAttempt((value) => value + 1),
  };
}
