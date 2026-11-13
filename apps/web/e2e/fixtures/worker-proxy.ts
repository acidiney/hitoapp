import { createServer, request as send } from "node:http";
import type { AddressInfo } from "node:net";

export async function workerProxy(target: string) {
  const upstream = new URL(target);
  const host =
    upstream.hostname === "localhost" ? "127.0.0.1" : upstream.hostname;
  const state = { changed: false, workerRequests: 0 };
  const server = createServer((incoming, outgoing) => {
    const isWorker = incoming.url === "/sw.js";
    const headers = { ...incoming.headers, host: upstream.host };
    if (isWorker) headers["accept-encoding"] = "identity";
    const forwarded = send(
      {
        host,
        port: upstream.port,
        path: incoming.url,
        method: incoming.method,
        headers,
      },
      (answer) => {
        if (!isWorker) {
          outgoing.writeHead(answer.statusCode ?? 502, answer.headers);
          answer.pipe(outgoing);
          return;
        }
        state.workerRequests++;
        const chunks: Buffer[] = [];
        answer.on("data", (chunk: Buffer) => chunks.push(chunk));
        answer.on("end", () => {
          const original = Buffer.concat(chunks);
          const body = state.changed
            ? Buffer.concat([original, Buffer.from("\nvoid 0;\n")])
            : original;
          outgoing.writeHead(answer.statusCode ?? 502, {
            ...answer.headers,
            "content-length": String(body.length),
            "cache-control": "no-store",
          });
          outgoing.end(body);
        });
      },
    );
    forwarded.on("error", () => outgoing.writeHead(502).end());
    incoming.pipe(forwarded);
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  return {
    origin: `http://127.0.0.1:${(server.address() as AddressInfo).port}`,
    state,
    close: async () => {
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      );
    },
  };
}
