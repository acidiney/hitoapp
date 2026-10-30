import { randomUUID } from "node:crypto";
import { createServer, type IncomingMessage } from "node:http";
import type { AddressInfo } from "node:net";
import { isDeepStrictEqual } from "node:util";
import type { Schema } from "../../src/shared/schema";

async function boundedBody(request: IncomingMessage) {
  const chunks: Buffer[] = [];
  let bytes = 0;
  for await (const chunk of request) {
    const value = Buffer.from(chunk);
    if ((bytes += value.length) > 2048)
      throw new Error("Owned recording request exceeded its bound");
    chunks.push(value);
  }
  return bytes
    ? (JSON.parse(Buffer.concat(chunks).toString()) as unknown)
    : null;
}

export async function recordingAttributionProvider(query: string) {
  if (
    process.env.HITO_RECORDING_ATTRIBUTION_E2E !== "1" ||
    process.env.PLAYWRIGHT_NO_COPY_PROMPT !== "1"
  )
    throw new Error("The disposable recording credits guard is required");
  const nonce = randomUUID();
  const token = `owned-recording-token-${nonce}`;
  const manifest = {
    id: "org.hito.browser.recording-attribution",
    name: `Owned recording catalogue ${nonce}`,
    protocol_version: 1,
    capabilities: ["search", "sources"],
  } satisfies Schema<"Manifest">;
  const creator = `owned-credit-creator-${nonce}-`.padEnd(2048, "a");
  const markup = `<img src="https://credit-remote.example.invalid/${nonce}" onerror="window.ownedCreditPayload=true">`;
  const first = {
    creators: [creator, markup, creator],
    provider_label:
      `<script>window.ownedCreditProvider=true</script>${nonce}`.padEnd(
        256,
        "p",
      ),
    content_url: `https://credits.example.invalid/recordings/${nonce}/first`,
  } satisfies Schema<"RecordingAttribution">;
  const second = {
    creators: [
      `owned-revised-creator-${nonce}`,
      markup,
      `owned-revised-creator-${nonce}`,
    ],
    provider_label: `owned-revised-provider-${nonce}`.padEnd(256, "r"),
    content_url: `https://credits.example.invalid/recordings/${nonce}/second`,
  } satisfies Schema<"RecordingAttribution">;
  const original = {
    id: `recording/owned-${nonce}`,
    title: `Owned credit recording ${nonce}`,
    artist: `Owned credit performer ${nonce}`,
    duration_ms: 12_000,
    release: null,
    disc_number: null,
    track_number: null,
    revision: "owned-unchanged-revision",
    attribution: first,
  } satisfies Schema<"ProviderRecording">;
  const replacement = {
    ...original,
    attribution: second,
  } satisfies Schema<"ProviderRecording">;
  const source = {
    id: `source/owned-${nonce}`,
    label: `Owned inert source ${nonce}`,
    ready: false,
  } satisfies Pick<Schema<"Source">, "id" | "label" | "ready">;
  const calls = {
    requests: 0,
    publicManifest: 0,
    privateManifest: 0,
    search: 0,
    detail: 0,
    sources: 0,
    unexpected: 0,
    unsafeHeaders: 0,
  };
  const active = new Set<Promise<void>>();
  let revised = false;
  let closing = false;
  const server = createServer((request, response) => {
    response.on("error", () => {
      if (!closing) calls.unexpected++;
    });
    const task = (async () => {
      const reject = (status = 400) => {
        calls.unexpected++;
        response.writeHead(status).end();
      };
      if (++calls.requests > 12) return reject(429);
      const path = request.url ?? "";
      const isManifest = path === "/credits/v1/manifest";
      const authorization = request.headers.authorization;
      if (
        request.headers.cookie !== undefined ||
        request.headers.range !== undefined ||
        request.headers["x-hito-plugin-protocol"] !== "1" ||
        (authorization !== `Bearer ${token}` &&
          !(isManifest && authorization === undefined))
      ) {
        calls.unsafeHeaders++;
        return reject(401);
      }
      const body = await boundedBody(request);
      const recording = revised ? replacement : original;
      let value: unknown;
      if (isManifest && request.method === "GET" && body === null) {
        if (authorization) calls.privateManifest++;
        else calls.publicManifest++;
        value = manifest;
      } else if (
        path === "/credits/v1/search" &&
        request.method === "POST" &&
        isDeepStrictEqual(body, { query, cursor: null, limit: 20 })
      ) {
        calls.search++;
        value = {
          items: [recording],
          next_cursor: null,
        } satisfies Schema<"SearchPage">;
      } else if (
        request.method === "POST" &&
        isDeepStrictEqual(body, { recording_id: original.id })
      ) {
        if (path === "/credits/v1/recordings/detail") {
          calls.detail++;
          value = recording;
        } else if (path === "/credits/v1/sources") {
          calls.sources++;
          value = { items: [source] };
        } else return reject(404);
      } else return reject(404);
      const bytes = JSON.stringify(value);
      if (Buffer.byteLength(bytes) > 16_384) return reject(500);
      response
        .writeHead(200, { "content-type": "application/json" })
        .end(bytes);
    })()
      .catch(() => {
        if (!closing) calls.unexpected++;
        response.destroy();
      })
      .finally(() => active.delete(task));
    active.add(task);
  });
  server.requestTimeout = 4000;
  server.headersTimeout = 4000;
  server.maxHeadersCount = 32;
  server.maxConnections = 8;
  server.setTimeout(4000, (socket) => socket.destroy());
  server.on("clientError", (_error, socket) => {
    if (!closing) calls.unexpected++;
    socket.destroy();
  });
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", reject);
      resolve();
    });
  });
  server.on("error", () => {
    if (!closing) calls.unexpected++;
  });
  const origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  return {
    token,
    manifest,
    original,
    replacement,
    source,
    first,
    second,
    calls,
    address: `${origin}/credits/`,
    privateValues: [
      token,
      origin,
      manifest.id,
      manifest.name,
      original.id,
      original.title,
      original.artist,
      source.id,
      source.label,
      "credit-remote.example.invalid",
      "ownedCreditPayload",
      "ownedCreditProvider",
      ...[first, second].flatMap((credit) => [
        ...credit.creators,
        credit.provider_label,
        credit.content_url,
      ]),
    ],
    revise: () => {
      revised = true;
    },
    async close() {
      closing = true;
      let timer: ReturnType<typeof setTimeout> | undefined;
      try {
        const stopped = new Promise<void>((resolve, reject) =>
          server.close((error) =>
            error
              ? reject(new Error("Owned credits provider close failed"))
              : resolve(),
          ),
        );
        server.closeAllConnections();
        await Promise.race([
          (async () => {
            await stopped;
            while (active.size) await Promise.allSettled([...active]);
          })(),
          new Promise<never>((_resolve, reject) => {
            timer = setTimeout(
              () => reject(new Error("Owned credits provider close timed out")),
              5000,
            );
          }),
        ]);
      } finally {
        clearTimeout(timer);
      }
    },
  };
}
