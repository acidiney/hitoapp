import { randomUUID } from "node:crypto";
import {
  createServer,
  type IncomingMessage,
  type ServerResponse,
} from "node:http";
import type { AddressInfo } from "node:net";
import type { components } from "../../src/generated";

type Schema = components["schemas"];

export async function metadataPreferencesProvider() {
  if (process.env.HITO_METADATA_PREFERENCES_E2E !== "1")
    throw new Error(
      "The disposable preferences fixture requires explicit opt-in",
    );
  const name = `Owned catalog ${randomUUID().slice(0, 8)}`;
  const manifests = ["first", "second"].map(
    (part) =>
      ({
        id: `org.hito.browser.metadata-preferences.${part}`,
        name,
        protocol_version: 1,
        capabilities: ["catalog"],
      }) satisfies Schema["Manifest"],
  );
  const calls = {
    manifest: [0, 0],
    descriptor: 0,
    profile: 0,
    audio: 0,
    download: 0,
    unexpected: 0,
    unsafeHeaders: 0,
    total: 0,
  };
  const active = new Set<Promise<void>>();
  async function handle(request: IncomingMessage, response: ServerResponse) {
    if (++calls.total > 8) {
      calls.unexpected++;
      response.writeHead(429).end();
      return;
    }
    if (
      request.headers.cookie ||
      request.headers.authorization ||
      request.headers["x-hito-plugin-protocol"] !== "1"
    ) {
      calls.unsafeHeaders++;
      response.writeHead(401).end();
      return;
    }
    let bytes = 0;
    for await (const chunk of request) {
      bytes += Buffer.byteLength(chunk);
      if (bytes > 1024) break;
    }
    const path = request.url ?? "";
    const index = ["/first/v1/manifest", "/second/v1/manifest"].indexOf(path);
    if (request.method === "GET" && bytes === 0 && index >= 0) {
      calls.manifest[index]++;
      response
        .writeHead(200, { "content-type": "application/json" })
        .end(JSON.stringify(manifests[index]));
      return;
    }
    if (path.endsWith("/v1/catalog/capabilities")) calls.descriptor++;
    else if (path.includes("artist") || path.includes("profile"))
      calls.profile++;
    else if (path.includes("audio")) calls.audio++;
    else if (path.includes("download")) calls.download++;
    else calls.unexpected++;
    response.writeHead(404).end("Owned catalog request was not permitted");
  }
  const server = createServer((request, response) => {
    const task = handle(request, response)
      .catch(() => {
        calls.unexpected++;
        response.destroy();
      })
      .finally(() => active.delete(task));
    active.add(task);
  });
  server.requestTimeout = 4000;
  server.headersTimeout = 4000;
  server.setTimeout(4000, (socket) => socket.destroy());
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", reject);
      resolve();
    });
  });
  const origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  return {
    name,
    manifests,
    calls,
    origin,
    addresses: [`${origin}/first/`, `${origin}/second/`],
    async close() {
      try {
        await new Promise<void>((resolve, reject) => {
          const timer = setTimeout(
            () => reject(new Error("Owned catalog close timed out")),
            3000,
          );
          server.close((error) => {
            clearTimeout(timer);
            if (error) reject(new Error("Owned catalog close failed"));
            else resolve();
          });
          server.closeAllConnections();
        });
      } finally {
        await Promise.allSettled([...active]);
      }
    },
  };
}
