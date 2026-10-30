import { randomUUID } from "node:crypto";
import { createServer, type IncomingMessage } from "node:http";
import type { AddressInfo } from "node:net";
import { isDeepStrictEqual } from "node:util";
import type { Schema } from "../../src/shared/schema";

export const publicSources = [
  { provider: "youtube", recording_id: "RankVideo01" },
  { provider: "jamendo", recording_id: "42" },
] as const;

async function body(request: IncomingMessage): Promise<unknown> {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of request) {
    const bytes = Buffer.from(chunk);
    if ((size += bytes.length) > 4096)
      throw new Error("Owned body exceeded its bound");
    chunks.push(bytes);
  }
  return size ? JSON.parse(Buffer.concat(chunks).toString("utf8")) : null;
}

export function ownedWav(frequency: number) {
  const samples = 12 * 16_000;
  const wav = Buffer.alloc(44 + samples * 2);
  wav.write("RIFF");
  wav.writeUInt32LE(wav.length - 8, 4);
  wav.write("WAVEfmt ", 8);
  wav.writeUInt32LE(16, 16);
  wav.writeUInt16LE(1, 20);
  wav.writeUInt16LE(1, 22);
  wav.writeUInt32LE(16_000, 24);
  wav.writeUInt32LE(32_000, 28);
  wav.writeUInt16LE(2, 32);
  wav.writeUInt16LE(16, 34);
  wav.write("data", 36);
  wav.writeUInt32LE(samples * 2, 40);
  for (let index = 0; index < samples; index++)
    wav.writeInt16LE(
      Math.round(4000 * Math.sin((2 * Math.PI * frequency * index) / 16_000)),
      44 + index * 2,
    );
  return wav;
}

export async function rankingProviders() {
  const nonce = randomUUID();
  const title = `private-ranking-tune-${nonce}`;
  const artist = `private-ranking-performer-${nonce}`;
  const states = [
    {
      service: "org.hito.youtube",
      recording: "RankVideo01",
      frequency: 440,
      ready: false,
      capabilities: ["sources"],
    },
    {
      service: "org.hito.jamendo",
      recording: "jamendo:track:v1:42",
      frequency: 660,
      ready: true,
      capabilities: ["search", "sources"],
    },
  ] as const;
  const active = new Set<Promise<void>>();
  let closing = false;
  const providers = states.map((state, index) => ({
    ...state,
    ready: Boolean(state.ready),
    token: `private-ranking-provider-token-${index}-${nonce}`,
    source: `private-ranking-source-${index}?signature=${nonce}`,
    label: `private-ranking-label-${index}-${nonce}`,
    wav: ownedWav(state.frequency),
    counts: {
      manifest: 0,
      search: 0,
      detail: 0,
      sources: 0,
      audio: 0,
      audioGet: 0,
      errors: 0,
    },
    detail: {
      id: state.recording,
      title,
      artist,
      duration_ms: 12_000,
    } satisfies Schema<"ProviderRecording">,
  }));
  const server = createServer((request, response) => {
    response.on("error", () => {
      if (!closing) providers[0].counts.errors++;
    });
    const task = (async () => {
      const path = new URL(request.url ?? "", "http://fixture.invalid");
      const match = /^\/provider\/(0|1)\/v1\/(.+)$/.exec(path.pathname);
      if (!match) throw new Error("Unknown owned provider route");
      const provider = providers[Number(match[1])];
      const operation = match[2];
      const manifest = operation === "manifest";
      if (
        request.headers.cookie !== undefined ||
        request.headers["x-hito-account"] !== undefined ||
        request.headers["x-hito-plugin-protocol"] !== "1" ||
        (request.headers.authorization !== `Bearer ${provider.token}` &&
          !(manifest && request.headers.authorization === undefined))
      )
        throw new Error("Owned provider received inappropriate authority");
      const input = await body(request);
      if (operation === "audio") {
        if (
          !["GET", "HEAD"].includes(request.method ?? "") ||
          input !== null ||
          path.searchParams.size !== 1 ||
          path.searchParams.get("source_id") !== provider.source
        )
          throw new Error("Owned provider audio input differs");
        provider.counts.audio++;
        if (request.method === "GET") provider.counts.audioGet++;
        let first = 0;
        let last = provider.wav.length - 1;
        const range =
          request.method === "HEAD" ? undefined : request.headers.range;
        if (range) {
          const bytes = /^bytes=(\d+)-(\d*)$/.exec(range);
          if (!bytes || Number(bytes[1]) >= provider.wav.length) {
            response
              .writeHead(416, {
                "content-range": `bytes */${provider.wav.length}`,
              })
              .end();
            return;
          }
          first = Number(bytes[1]);
          if (bytes[2]) last = Math.min(last, Number(bytes[2]));
          if (last < first) throw new Error("Invalid owned audio range");
        }
        response
          .writeHead(range ? 206 : 200, {
            "content-type": "audio/wav",
            "content-length": last - first + 1,
            "accept-ranges": "bytes",
            ...(range
              ? {
                  "content-range": `bytes ${first}-${last}/${provider.wav.length}`,
                }
              : {}),
          })
          .end(
            request.method === "HEAD"
              ? undefined
              : provider.wav.subarray(first, last + 1),
          );
        return;
      }
      if (path.searchParams.size) throw new Error("Unexpected owned query");
      let value: unknown;
      if (manifest && request.method === "GET" && input === null) {
        provider.counts.manifest++;
        value = {
          id: provider.service,
          name: `Owned ranking provider ${Number(match[1])}`,
          protocol_version: 1,
          capabilities: [...provider.capabilities],
          content_policy: { source_persistence: "playback_only" },
        } satisfies Schema<"Manifest">;
      } else if (
        operation === "search" &&
        request.method === "POST" &&
        Number(match[1]) === 1 &&
        isDeepStrictEqual(input, {
          query: `${artist} ${title}`,
          cursor: null,
          limit: 20,
        })
      ) {
        provider.counts.search++;
        value = { items: [provider.detail], next_cursor: null };
      } else if (
        request.method === "POST" &&
        isDeepStrictEqual(input, { recording_id: provider.recording })
      ) {
        if (operation === "recordings/detail") {
          provider.counts.detail++;
          value = provider.detail;
        } else if (operation === "sources") {
          provider.counts.sources++;
          value = {
            items: [
              {
                id: provider.source,
                label: provider.label,
                ready: provider.ready,
              },
            ],
          };
        } else throw new Error("Unknown owned operation");
      } else throw new Error("Owned provider body differs");
      const bytes = Buffer.from(JSON.stringify(value));
      if (bytes.length > 8192)
        throw new Error("Owned provider reply exceeded its bound");
      response
        .writeHead(200, {
          "content-type": "application/json",
          "content-length": bytes.length,
        })
        .end(bytes);
    })()
      .catch(() => {
        providers[0].counts.errors++;
        if (!response.headersSent) response.writeHead(500).end();
        else response.destroy();
      })
      .finally(() => active.delete(task));
    active.add(task);
  });
  server.maxConnections = 8;
  server.maxHeadersCount = 32;
  server.requestTimeout = 4000;
  server.headersTimeout = 4000;
  server.setTimeout(4000, (socket) => socket.destroy());
  server.on("clientError", (_error, socket) => {
    if (!closing) providers[0].counts.errors++;
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
    if (!closing) providers[0].counts.errors++;
  });
  const origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  return {
    providers,
    origin,
    address: (index: number) => `${origin}/provider/${index}/`,
    snapshot: () => providers.map((provider) => ({ ...provider.counts })),
    privateValues: providers
      .flatMap((provider) => [provider.token, provider.source, provider.label])
      .concat(title, artist, origin),
    async close() {
      closing = true;
      server.closeAllConnections();
      await new Promise<void>((resolve) => server.close(() => resolve()));
      await Promise.all(active);
      if (active.size)
        throw new Error("Owned provider callbacks did not retire");
    },
  };
}
