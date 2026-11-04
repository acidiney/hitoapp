export function combineSignals(
  caller: AbortSignal | undefined,
  limit: AbortSignal,
) {
  if (!caller) return { signal: limit, release: () => {} };
  if (typeof AbortSignal.any === "function")
    return { signal: AbortSignal.any([caller, limit]), release: () => {} };
  const joined = new AbortController();
  const abort = () => joined.abort();
  if (caller.aborted || limit.aborted) joined.abort();
  caller.addEventListener("abort", abort, { once: true });
  limit.addEventListener("abort", abort, { once: true });
  return {
    signal: joined.signal,
    release: () => {
      caller.removeEventListener("abort", abort);
      limit.removeEventListener("abort", abort);
    },
  };
}
