export class ObservationError extends Error {
  constructor() {
    super("The playback observation is invalid.");
  }
}

export async function readObservations<T>(
  response: Response,
  signal: AbortSignal,
  receive: (value: T) => void,
) {
  if (
    !response.headers.get("content-type")?.startsWith("text/event-stream") ||
    !response.body
  )
    throw new ObservationError();
  const reader = response.body.getReader();
  const decoder = new TextDecoder("utf-8", { fatal: true });
  let buffered = "";
  const cancel = () => {
    void reader.cancel().catch(() => {});
  };
  signal.addEventListener("abort", cancel, { once: true });
  try {
    while (!signal.aborted) {
      const chunk = await reader.read();
      if (signal.aborted || chunk.done) return;
      try {
        buffered = (buffered + decoder.decode(chunk.value, { stream: true })).replace(/\r\n/g, "\n");
      } catch {
        throw new ObservationError();
      }
      let end: number;
      while ((end = buffered.indexOf("\n\n")) >= 0) {
        if (end > 65536) throw new ObservationError();
        const frame = buffered.slice(0, end);
        buffered = buffered.slice(end + 2);
        const data = frame
          .split("\n")
          .filter((line) => line.startsWith("data:"))
          .map((line) => line.slice(5).replace(/^ /, ""))
          .join("\n");
        if (data) {
          let value: T;
          try {
            value = JSON.parse(data);
            if (value === null || typeof value !== "object" || Array.isArray(value)) throw new ObservationError();
          } catch {
            throw new ObservationError();
          }
          if (!signal.aborted) receive(value);
        }
      }
      if (buffered.length > 65536) throw new ObservationError();
    }
  } finally {
    signal.removeEventListener("abort", cancel);
    await reader.cancel().catch(() => {});
    reader.releaseLock();
  }
}
