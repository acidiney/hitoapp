import { randomUUID } from "node:crypto";
import { createServer, type IncomingMessage } from "node:http";
import type { AddressInfo } from "node:net";
import type { Schema } from "../../src/shared/schema";

class ProtocolFailure extends Error {
  constructor(readonly status = 400) {
    super("Owned fixture protocol rejected");
  }
}

async function readBody(request: IncomingMessage) {
  const chunks: Buffer[] = [];
  let bytes = 0;
  for await (const chunk of request) {
    const value = Buffer.from(chunk);
    bytes += value.length;
    if (bytes > 8192) throw new ProtocolFailure(413);
    chunks.push(value);
  }
  if (bytes === 0) return null;
  try {
    const value: unknown = JSON.parse(Buffer.concat(chunks).toString("utf8"));
    if (value === null || typeof value !== "object" || Array.isArray(value))
      throw new ProtocolFailure();
    return value as Record<string, unknown>;
  } catch {
    throw new ProtocolFailure();
  }
}

function exactKeys(value: Record<string, unknown> | null, keys: string[]) {
  if (!value || Object.keys(value).sort().join(",") !== keys.sort().join(","))
    throw new ProtocolFailure();
  return value;
}

function ownedWav() {
  const rate = 16000;
  const samples = rate * 12;
  const wav = Buffer.alloc(44 + samples * 2);
  wav.write("RIFF", 0);
  wav.writeUInt32LE(wav.length - 8, 4);
  wav.write("WAVEfmt ", 8);
  wav.writeUInt32LE(16, 16);
  wav.writeUInt16LE(1, 20);
  wav.writeUInt16LE(1, 22);
  wav.writeUInt32LE(rate, 24);
  wav.writeUInt32LE(rate * 2, 28);
  wav.writeUInt16LE(2, 32);
  wav.writeUInt16LE(16, 34);
  wav.write("data", 36);
  wav.writeUInt32LE(samples * 2, 40);
  for (let index = 0; index < samples; index++) {
    wav.writeInt16LE(
      Math.round(4000 * Math.sin((2 * Math.PI * 440 * index) / rate)),
      44 + index * 2,
    );
  }
  return wav;
}

export async function automaticSourceFixture() {
  if (
    process.env.HITO_AUTOMATIC_SOURCE_E2E !== "1" ||
    process.env.PLAYWRIGHT_NO_COPY_PROMPT !== "1"
  )
    throw new Error("The isolated automatic source privacy guard is required");
  const suffix = randomUUID().slice(0, 8);
  const artist = "Hito owned fixture artist";
  const title = `Orbit ${suffix}`;
  const release = { id: "release/studio", title: "Studio sessions", artist };
  const original = {
    id: "metadata/original",
    title,
    artist,
    duration_ms: 12000,
    release,
    revision: "metadata-one",
  } satisfies Schema<"ProviderRecording">;
  const suitable = {
    ...original,
    id: "audio/studio",
    release: { ...release, id: "audio/release/studio" },
    revision: "audio-one",
  } satisfies Schema<"ProviderRecording">;
  const live = {
    ...suitable,
    id: "audio/live",
    release: {
      id: "audio/release/live",
      title: "Live at the fixture hall",
      artist,
    },
  } satisfies Schema<"ProviderRecording">;
  const candidates: Schema<"ProviderRecording">[] = [
    live,
    { ...suitable, id: "audio/cover", artist: "Another owned performer" },
    { ...suitable, id: "audio/remix", title: `${title} (Fixture Remix)` },
    { ...suitable, id: "audio/long", duration_ms: 60000 },
    suitable,
  ];
  const recordings = new Map(candidates.map((item) => [item.id, item]));
  const audio = ownedWav();
  const sourceFor = (recording: string) => `source/${recording}`;
  const queries: string[] = [];
  const counts = {
    search: 0,
    details: 0,
    sources: 0,
    audio: 0,
    audioGet: 0,
    unsuitableAudio: 0,
    protocol: 0,
    unexpected: 0,
    handler: 0,
  };
  const token = `owned-source-fixture-${suffix}`;
  const reference = `owned-playlist-${suffix}`;
  const playlist = {
    id: "playlist/ordered",
    name: `Automatic source ${suffix}`,
    snapshot: "one",
    total_count: 2,
    attribution: { label: "Owned metadata fixture" },
  };
  const items = {
    playlist_id: playlist.id,
    snapshot: playlist.snapshot,
    items: [
      {
        occurrence_id: "first",
        item: { kind: "recording" as const, recording: original },
      },
      {
        occurrence_id: "duplicate",
        item: { kind: "recording" as const, recording: original },
      },
    ],
    next_cursor: null,
  };
  const server = createServer(async (request, response) => {
    response.on("error", () => counts.handler++);
    try {
      if (
        request.headers.authorization !== `Bearer ${token}` ||
        request.headers["x-hito-plugin-protocol"] !== "1" ||
        request.headers.cookie !== undefined
      )
        throw new ProtocolFailure(401);
      const url = new URL(request.url ?? "/", "http://fixture.invalid");
      const body = await readBody(request);
      if (url.pathname === "/audio/v1/audio") {
        const source = url.searchParams.get("source_id");
        if (
          !["GET", "HEAD"].includes(request.method ?? "") ||
          body !== null ||
          url.searchParams.size !== 1 ||
          !candidates.some((item) => sourceFor(item.id) === source)
        )
          throw new ProtocolFailure(404);
        let first = 0;
        let last = audio.length - 1;
        const range = request.headers.range;
        if (range) {
          const match = /^bytes=(\d+)-(\d*)$/.exec(range);
          if (
            !match ||
            !Number.isSafeInteger(Number(match[1])) ||
            Number(match[1]) >= audio.length ||
            (match[2] !== "" &&
              (!Number.isSafeInteger(Number(match[2])) ||
                Number(match[2]) < Number(match[1])))
          )
            throw new ProtocolFailure(416);
          first = Number(match[1]);
          if (match[2]) last = Math.min(last, Number(match[2]));
        }
        counts.audio++;
        if (request.method === "GET") counts.audioGet++;
        if (source !== sourceFor(suitable.id)) counts.unsuitableAudio++;
        response.writeHead(range ? 206 : 200, {
          "content-type": "audio/wav",
          "content-length": last - first + 1,
          "accept-ranges": "bytes",
          ...(range
            ? { "content-range": `bytes ${first}-${last}/${audio.length}` }
            : {}),
        });
        response.end(
          request.method === "HEAD"
            ? undefined
            : audio.subarray(first, last + 1),
        );
        return;
      }
      if (url.searchParams.size !== 0) throw new ProtocolFailure();
      let reply: unknown;
      if (
        ["/metadata/v1/manifest", "/audio/v1/manifest"].includes(url.pathname)
      ) {
        if (request.method !== "GET" || body !== null)
          throw new ProtocolFailure();
        reply =
          url.pathname === "/metadata/v1/manifest"
            ? ({
                id: "org.hito.owned-metadata-fixture",
                name: "Owned metadata fixture",
                protocol_version: 1,
                capabilities: ["playlists"],
              } satisfies Schema<"Manifest">)
            : ({
                id: "org.hito.owned-audio-fixture",
                name: "Owned audio fixture",
                protocol_version: 1,
                capabilities: ["search", "sources"],
              } satisfies Schema<"Manifest">);
      } else if (
        [
          "/metadata/v1/playlists/detail",
          "/metadata/v1/playlists/items",
          "/audio/v1/search",
          "/audio/v1/recordings/detail",
          "/audio/v1/sources",
        ].includes(url.pathname)
      ) {
        if (request.method !== "POST") throw new ProtocolFailure(405);
        if (url.pathname === "/metadata/v1/playlists/detail") {
          const input = exactKeys(
            body,
            body?.kind === "reference"
              ? ["kind", "reference"]
              : ["kind", "playlist_id"],
          );
          if (
            !(
              (input.kind === "reference" && input.reference === reference) ||
              (input.kind === "canonical" && input.playlist_id === playlist.id)
            )
          )
            throw new ProtocolFailure();
          reply = playlist;
        } else if (url.pathname === "/metadata/v1/playlists/items") {
          const input = exactKeys(body, [
            "playlist_id",
            "snapshot",
            "cursor",
            "limit",
          ]);
          if (
            input.playlist_id !== playlist.id ||
            input.snapshot !== playlist.snapshot ||
            input.cursor !== null ||
            input.limit !== 100
          )
            throw new ProtocolFailure();
          reply = items;
        } else if (url.pathname === "/audio/v1/search") {
          const input = exactKeys(body, ["query", "cursor", "limit"]);
          if (
            typeof input.query !== "string" ||
            !input.query.trim() ||
            Buffer.byteLength(input.query) > 512 ||
            /[\u0000-\u001f\u007f]/.test(input.query) ||
            input.cursor !== null ||
            typeof input.limit !== "number" ||
            !Number.isInteger(input.limit) ||
            input.limit < 1 ||
            input.limit > 20
          )
            throw new ProtocolFailure();
          counts.search++;
          queries.push(input.query);
          reply = {
            items: candidates.slice(0, input.limit),
            next_cursor: null,
          };
        } else {
          const input = exactKeys(body, ["recording_id"]);
          if (
            typeof input.recording_id !== "string" ||
            !recordings.has(input.recording_id)
          )
            throw new ProtocolFailure(404);
          if (url.pathname === "/audio/v1/recordings/detail") {
            counts.details++;
            reply = recordings.get(input.recording_id);
          } else {
            counts.sources++;
            reply = {
              items: [
                {
                  id: sourceFor(input.recording_id),
                  label: "Owned original",
                  ready: true,
                },
              ],
            };
          }
        }
      } else {
        counts.unexpected++;
        response.writeHead(404).end();
        return;
      }
      response
        .writeHead(200, { "content-type": "application/json" })
        .end(JSON.stringify(reply));
    } catch (error) {
      const protocol = error instanceof ProtocolFailure;
      if (protocol) counts.protocol++;
      else counts.handler++;
      try {
        if (!response.destroyed) {
          if (!response.headersSent)
            response.writeHead(protocol ? error.status : 500);
          response.end();
        }
      } catch {
        counts.handler++;
      }
    }
  });
  server.requestTimeout = 5000;
  server.headersTimeout = 5000;
  server.maxHeadersCount = 32;
  server.on("clientError", (_error, socket) => {
    counts.protocol++;
    socket.destroy();
  });
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", reject);
      resolve();
    });
  });
  server.on("error", () => counts.handler++);
  const address = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  return {
    metadataAddress: `${address}/metadata/`,
    audioAddress: `${address}/audio/`,
    token,
    reference,
    playlist,
    original,
    suitable,
    live,
    suitableSource: sourceFor(suitable.id),
    counts,
    privateValues: [
      token,
      reference,
      playlist.name,
      original.title,
      original.artist,
      original.id,
      ...candidates.flatMap((item) => [item.id, sourceFor(item.id)]),
    ],
    queriesUseMusicalIdentity: (excluded: string[]) =>
      queries.length > 0 &&
      queries.every(
        (query) =>
          query.includes(original.title) &&
          query.includes(original.artist) &&
          !query.includes(original.id) &&
          excluded.every((value) => !query.includes(value)),
      ),
    close: async () => {
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      );
    },
  };
}
