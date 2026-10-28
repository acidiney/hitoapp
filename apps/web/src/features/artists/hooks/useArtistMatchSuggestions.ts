import { useCallback, useEffect, useRef, useState } from "react";
import { artistMatchingApi } from "../api/artistMatchingApi";
import { log } from "../../../shared/log";
import type { ArtistMatchSuggestions } from "../../../shared/types";

const DEADLINE_MS = 15000;

type View = {
  status: "loading" | "ready" | "failed";
  suggestions: ArtistMatchSuggestions | null;
  busy: boolean;
};

export function useArtistMatchSuggestions(account: string, artist: string) {
  const [view, setView] = useState<View>({
    status: "loading",
    suggestions: null,
    busy: false,
  });
  const active = useRef<AbortController | null>(null);

  const load = useCallback(
    async (
      work: (signal: AbortSignal) => Promise<ArtistMatchSuggestions>,
      reason: string,
    ) => {
      active.current?.abort();
      const controller = new AbortController();
      active.current = controller;
      const timer = setTimeout(() => controller.abort(), DEADLINE_MS);
      const started = performance.now();
      setView((value) => ({ ...value, busy: true }));
      try {
        const suggestions = await work(controller.signal);
        if (controller.signal.aborted) return;
        if (suggestions.artist_id !== artist) throw new Error("Wrong credit");
        setView({ status: "ready", suggestions, busy: false });
        log.debug("artist_match_suggestions.loaded", {
          reason,
          count: suggestions.items.length,
          duration_ms: Math.round(performance.now() - started),
        });
      } catch {
        if (active.current === controller && !controller.signal.aborted)
          setView((value) => ({ ...value, status: "failed", busy: false }));
      } finally {
        clearTimeout(timer);
      }
    },
    [artist],
  );

  const refresh = useCallback(
    () =>
      load(
        (signal) => artistMatchingApi.suggestions(account, artist, signal),
        "refresh",
      ),
    [account, artist, load],
  );

  useEffect(() => {
    setView({ status: "loading", suggestions: null, busy: false });
    void refresh();
    return () => active.current?.abort();
  }, [refresh]);

  const dismiss = useCallback(
    (group: string) =>
      load(
        (signal) =>
          artistMatchingApi.dismiss(
            account,
            artist,
            { group_id: group },
            signal,
          ),
        "dismiss",
      ),
    [account, artist, load],
  );

  return { ...view, refresh, dismiss };
}
