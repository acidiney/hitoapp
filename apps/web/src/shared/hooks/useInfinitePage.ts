import { useEffect, useRef, useState } from "react";
import { useRemember } from "@inertiajs/react";
import { errorMessage } from "../format";
import { log } from "../log";

export type ListPage<T> = {
  items: T[];
  total: number;
  offset: number;
  limit: number;
};

type Loaded<T> = { scope: string; offset: number; items: T[] };

export function useInfinitePage<T extends { id: string }>({
  memory,
  scope,
  initial,
  fetchPage,
}: {
  memory: string;
  scope: string;
  initial: ListPage<T>;
  fetchPage(offset: number, signal: AbortSignal): Promise<ListPage<T>>;
}) {
  const [remembered, setRemembered] = useRemember<Loaded<T>>(
    { scope, offset: initial.offset, items: initial.items },
    memory,
  );
  const [state, setState] = useState({ loading: false, error: "" });
  const controller = useRef<AbortController | null>(null);
  const fresh =
    remembered.scope === scope && remembered.offset === initial.offset;
  const loaded = fresh
    ? remembered
    : { scope, offset: initial.offset, items: initial.items };
  const first = useRef(initial);
  useEffect(() => {
    if (first.current === initial) return;
    first.current = initial;
    controller.current?.abort();
    setState({ loading: false, error: "" });
    setRemembered((previous) => {
      const same =
        previous.scope === scope && previous.offset === initial.offset;
      const tail = same ? previous.items.slice(initial.items.length) : [];
      return {
        scope,
        offset: initial.offset,
        items: [...initial.items, ...tail],
      };
    });
  }, [initial, scope, setRemembered]);
  useEffect(() => () => controller.current?.abort(), []);

  const nextOffset = loaded.offset + loaded.items.length;
  const hasMore = nextOffset < initial.total;

  async function loadMore() {
    if (state.loading || !hasMore) return;
    controller.current?.abort();
    const request = new AbortController();
    controller.current = request;
    const started = performance.now();
    let outcome = "loaded";
    setState({ loading: true, error: "" });
    try {
      const page = await fetchPage(nextOffset, request.signal);
      if (request.signal.aborted) return;
      setRemembered((previous) => {
        const base = previous.scope === scope ? previous : loaded;
        const known = new Set(base.items.map((item) => item.id));
        return {
          ...base,
          items: [
            ...base.items,
            ...page.items.filter((item) => !known.has(item.id)),
          ],
        };
      });
      setState({ loading: false, error: "" });
    } catch (error) {
      if (request.signal.aborted) return;
      outcome = "failed";
      setState({ loading: false, error: errorMessage(error) });
    } finally {
      log.debug("pagination.load_more", {
        memory,
        reason: outcome,
        index: nextOffset,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  return {
    items: loaded.items,
    total: initial.total,
    hasMore,
    loading: state.loading,
    error: state.error,
    loadMore,
  };
}
