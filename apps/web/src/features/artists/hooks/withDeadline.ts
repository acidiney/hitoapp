export class DeadlineError extends Error {}

export async function withDeadline<T>(
  parent: AbortSignal,
  milliseconds: number,
  run: (signal: AbortSignal) => Promise<T>,
): Promise<T> {
  const controller = new AbortController();
  let stop!: (error: Error) => void;
  const stopped = new Promise<never>((_, reject) => {
    stop = reject;
  });
  const cancel = () => {
    const error = new DOMException("The request was cancelled", "AbortError");
    controller.abort(error);
    stop(error);
  };
  parent.addEventListener("abort", cancel, { once: true });
  const timer = setTimeout(() => {
    const error = new DeadlineError("This request timed out. Try again.");
    controller.abort(error);
    stop(error);
  }, milliseconds);
  if (parent.aborted) cancel();
  try {
    return await Promise.race([run(controller.signal), stopped]);
  } finally {
    clearTimeout(timer);
    parent.removeEventListener("abort", cancel);
  }
}
