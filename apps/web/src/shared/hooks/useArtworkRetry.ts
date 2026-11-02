import { useCallback, useEffect, useState } from "react";

type RetryState = {
  source: string;
  attempt: number;
  waiting: boolean;
  exhausted: boolean;
};

function fresh(source: string): RetryState {
  return { source, attempt: 0, waiting: false, exhausted: false };
}

export function useArtworkRetry(delays: readonly number[], source: string) {
  const [state, setState] = useState(() => fresh(source));
  const current = state.source === source ? state : fresh(source);
  const { attempt, waiting, exhausted } = current;

  useEffect(() => {
    if (!waiting) return;
    const timer = setTimeout(
      () =>
        setState({
          source,
          attempt: attempt + 1,
          waiting: false,
          exhausted: false,
        }),
      delays[attempt],
    );
    return () => clearTimeout(timer);
  }, [waiting, attempt, source, delays]);

  const miss = useCallback(() => {
    setState((previous) => {
      const base = previous.source === source ? previous : fresh(source);
      if (base.waiting || base.exhausted) return base;
      return base.attempt < delays.length
        ? { ...base, waiting: true }
        : { ...base, exhausted: true };
    });
  }, [delays, source]);

  return { attempt, waiting, exhausted, miss };
}
