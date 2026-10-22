import { vi } from "vitest";

export function discographyHttp() {
  const replies: Array<(signal: AbortSignal) => Promise<Response> | Response> =
    [];
  const responses: Response[] = [];
  const cleanup: Array<() => void> = [];
  const fetch = vi.fn(async (_path: string, options?: RequestInit) => {
    const next = replies.shift();
    if (!next || !options?.signal || options.method !== "POST")
      throw new Error(
        "Unexpected request outside the owned discography lifecycle",
      );
    return next(options.signal);
  });
  vi.stubGlobal("fetch", fetch);
  return {
    fetch,
    responses,
    reply(value: unknown, status = 200) {
      const response = Response.json(value, { status });
      responses.push(response);
      replies.push(() => response);
    },
    hold(mode: "headers" | "body", value: unknown, status = 200) {
      const bytes = new TextEncoder().encode(JSON.stringify(value));
      if (bytes.length > 65_536)
        throw new Error("The owned response exceeds its fixture limit");
      let enter!: () => void;
      const entered = new Promise<void>((resolve) => {
        enter = resolve;
      });
      let released = false;
      let controller: ReadableStreamDefaultController<Uint8Array> | null = null;
      const response =
        mode === "headers"
          ? Response.json(value, { status })
          : new Response(
              new ReadableStream<Uint8Array>({
                start(body) {
                  controller = body;
                  body.enqueue(bytes.slice(0, 1));
                },
                pull() {
                  enter();
                },
                cancel() {
                  released = true;
                },
              }),
              { status, headers: { "content-type": "application/json" } },
            );
      responses.push(response);
      let releaseHeaders!: (response: Response) => void;
      const headers = new Promise<Response>((resolve) => {
        releaseHeaders = resolve;
      });
      let signal: AbortSignal | null = null;
      let aborts = 0;
      const aborted = () => {
        aborts++;
      };
      const release = () => {
        if (released) return;
        released = true;
        if (mode === "headers") releaseHeaders(response);
        else {
          controller!.enqueue(bytes.slice(1));
          controller!.close();
        }
      };
      replies.push((requestSignal) => {
        signal = requestSignal;
        signal.addEventListener("abort", aborted, { once: true });
        if (mode === "headers") {
          enter();
          return headers;
        }
        return response;
      });
      cleanup.push(() => {
        release();
        signal?.removeEventListener("abort", aborted);
      });
      return {
        entered,
        release,
        signal: () => signal,
        aborts: () => aborts,
        response,
      };
    },
    close() {
      for (const finish of cleanup) finish();
    },
  };
}
