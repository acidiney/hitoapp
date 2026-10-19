import { randomUUID } from "node:crypto";
import { createServer, type IncomingMessage } from "node:http";
import type { AddressInfo } from "node:net";
import { isDeepStrictEqual } from "node:util";
import type { components } from "../../src/generated";

type Schema = components["schemas"];

async function body(request: IncomingMessage) {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of request) {
    if ((size += Buffer.byteLength(chunk)) > 4096)
      throw new Error("Owned lyric request exceeded its bound");
    chunks.push(Buffer.from(chunk));
  }
  return size
    ? (JSON.parse(Buffer.concat(chunks).toString()) as unknown)
    : null;
}

export async function recordingLyricsProvider(query: string) {
  if (process.env.HITO_RECORDING_LYRICS_E2E !== "1")
    throw new Error("A disposable lyric provider requires explicit opt-in");
  const nonce = randomUUID();
  const token = `owned-lyric-token-${nonce}`;
  const manifest = {
    id: "org.hito.browser.recording-lyrics",
    name: `Owned lyric provider ${nonce}`,
    protocol_version: 1,
    capabilities: ["search", "sources", "lyrics"],
    content_policy: { source_persistence: "playback_only" },
  } satisfies Schema["Manifest"];
  const recordings = ["first", "second"].map((name) => ({
    id: `recording/${name}?owned=${nonce}`,
    title: `Owned lyric ${name} ${nonce}`,
    artist: `Owned lyric performer ${nonce}`,
    duration_ms: 12_000,
    release: null,
    disc_number: null,
    track_number: null,
    revision: `owned-recording-revision:"${name}"`,
  })) satisfies Schema["ProviderRecording"][];
  const attribution = {
    creators: [`Owned lyric creator ${nonce}`, `Owned lyric creator ${nonce}`],
    provider_label: `Owned lyric credits ${nonce}`,
    content_url: `https://credits.example.invalid/lyrics/${nonce}`,
  } satisfies Schema["RecordingAttribution"];
  const lines = [
    { start_ms: 0, end_ms: 1200, text: `Owned lyric opening ${nonce}` },
    {
      start_ms: 1500,
      end_ms: 2400,
      text: `<img src="https://lyrics.example.invalid/${nonce}" onerror="window.ownedLyricPayload=true">`,
    },
  ];
  const lyric = {
    id: `owned-lyrics-${nonce}`,
    rendition_id: `owned-rendition-${nonce}`,
    revision: `owned-lyric-revision:"${nonce}"`,
    rendition_label: `Owned static rendition ${nonce}`,
    language: "en",
    duration_ms: 3000,
    attribution,
    lines,
  } satisfies Schema["TimedLyrics"];
  const calls = {
    total: 0,
    publicManifest: 0,
    privateManifest: 0,
    search: 0,
    detail: 0,
    sources: 0,
    descriptor: 0,
    lyrics: 0,
    held: 0,
    dropped: 0,
    released: 0,
    unexpected: 0,
    unsafeHeaders: 0,
  };
  const lyricRequests: unknown[] = [];
  const tasks = new Set<Promise<void>>();
  const held = new Set<() => void>();
  let closing = false;
  let mode: "ready" | "null" | "held" = "ready";
  const server = createServer((request, response) => {
    const task = (async () => {
      const reject = (status = 400) => {
        calls.unexpected++;
        response.writeHead(status).end();
      };
      response.on("error", () => {
        if (!closing) calls.unexpected++;
      });
      if (++calls.total > 40) return reject(429);
      const path = request.url ?? "";
      const isManifest = path === "/lyrics/v1/manifest";
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
      const sent = await body(request);
      let reply: unknown;
      if (request.method === "GET" && sent === null && isManifest) {
        calls[authorization ? "privateManifest" : "publicManifest"]++;
        reply = manifest;
      } else if (
        request.method === "GET" &&
        sent === null &&
        path === "/lyrics/v1/lyrics/capabilities"
      ) {
        calls.descriptor++;
        reply = { lyrics_version: 1 };
      } else if (request.method === "POST" && path === "/lyrics/v1/search") {
        const index = recordings.findIndex((_recording, index) =>
          isDeepStrictEqual(sent, {
            query: `${query}-${index}`,
            cursor: null,
            limit: 20,
          }),
        );
        if (index < 0) return reject();
        calls.search++;
        reply = { items: [recordings[index]], next_cursor: null };
      } else if (request.method === "POST") {
        const index = recordings.findIndex((recording) =>
          isDeepStrictEqual(sent, { recording_id: recording.id }),
        );
        if (path === "/lyrics/v1/recordings/detail" && index >= 0) {
          calls.detail++;
          reply = recordings[index];
        } else if (path === "/lyrics/v1/sources" && index >= 0) {
          calls.sources++;
          reply = { items: [] };
        } else if (path === "/lyrics/v1/lyrics") {
          const selected = recordings.find((recording) =>
            isDeepStrictEqual(sent, {
              recording: { id: recording.id, revision: recording.revision },
            }),
          );
          if (!selected || lyricRequests.length >= 8) return reject();
          calls.lyrics++;
          lyricRequests.push(structuredClone(sent));
          reply = {
            recording: { id: selected.id, revision: selected.revision },
            lyrics: mode === "null" ? null : lyric,
          } satisfies Schema["LyricsResponse"];
          if (mode === "held") {
            const bytes = JSON.stringify(reply);
            calls.held++;
            response.writeHead(200, { "content-type": "application/json" });
            response.write(bytes.slice(0, 1));
            await new Promise<void>((resolve) => {
              const release = () => {
                held.delete(release);
                if (!response.destroyed) {
                  calls.released++;
                  response.end(bytes.slice(1));
                }
                resolve();
              };
              held.add(release);
              response.once("close", () => {
                if (held.delete(release)) calls.dropped++;
                resolve();
              });
            });
            return;
          }
        } else return reject(404);
      } else return reject(404);
      const bytes = JSON.stringify(reply);
      if (Buffer.byteLength(bytes) > 16_384) return reject(500);
      response
        .writeHead(200, { "content-type": "application/json" })
        .end(bytes);
    })()
      .catch(() => {
        if (!closing) calls.unexpected++;
        response.destroy();
      })
      .finally(() => tasks.delete(task));
    tasks.add(task);
  });
  server.requestTimeout = 4000;
  server.headersTimeout = 4000;
  server.maxHeadersCount = 32;
  server.maxConnections = 8;
  server.setTimeout(12_000, (socket) => socket.destroy());
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
    recordings,
    lyric,
    calls,
    lyricRequests,
    address: `${origin}/lyrics/`,
    privateValues: [
      token,
      origin,
      manifest.id,
      manifest.name,
      query,
      ...recordings.flatMap((recording) => [
        recording.id,
        recording.title,
        recording.artist,
        recording.revision,
      ]),
      lyric.id,
      lyric.rendition_id,
      lyric.revision,
      lyric.rendition_label,
      ...lines.map((line) => line.text),
      ...attribution.creators,
      attribution.provider_label,
      attribution.content_url,
      "ownedLyricPayload",
    ],
    setMode(value: typeof mode) {
      mode = value;
    },
    async close() {
      closing = true;
      for (const release of held) release();
      const stopped = new Promise<void>((resolve, reject) =>
        server.close((error) =>
          error
            ? reject(new Error("Owned lyric provider close failed"))
            : resolve(),
        ),
      );
      server.closeAllConnections();
      let timer: ReturnType<typeof setTimeout> | undefined;
      try {
        await Promise.race([
          Promise.all([stopped, ...tasks]),
          new Promise<never>((_resolve, reject) => {
            timer = setTimeout(
              () =>
                reject(
                  new Error("Owned lyric provider close exceeded its deadline"),
                ),
              4000,
            );
          }),
        ]);
      } finally {
        clearTimeout(timer);
      }
    },
  };
}
