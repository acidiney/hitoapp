import { randomUUID } from "node:crypto";
import { createServer, type IncomingMessage } from "node:http";
import type { AddressInfo } from "node:net";
import type { components } from "../../src/generated";

type Schema = components["schemas"];
class ProtocolFailure extends Error {
  constructor(readonly status = 400) {
    super("Owned next-entry fixture rejected the protocol");
  }
}

async function body(request: IncomingMessage) {
  const chunks: Buffer[] = [];
  let bytes = 0;
  for await (const chunk of request) {
    const value = Buffer.from(chunk);
    bytes += value.length;
    if (bytes > 8192) throw new ProtocolFailure(413);
    chunks.push(value);
  }
  if (!bytes) return null;
  try {
    const value: unknown = JSON.parse(Buffer.concat(chunks).toString("utf8"));
    if (!value || typeof value !== "object" || Array.isArray(value))
      throw new ProtocolFailure();
    return value as Record<string, unknown>;
  } catch {
    throw new ProtocolFailure();
  }
}

function fields(value: Record<string, unknown> | null, keys: string[]) {
  if (!value || Object.keys(value).sort().join() !== [...keys].sort().join())
    throw new ProtocolFailure();
  return value;
}

function wav(seconds: number, frequency: number) {
  const rate = 16000;
  const samples = rate * seconds;
  const audio = Buffer.alloc(44 + samples * 2);
  audio.write("RIFF", 0);
  audio.writeUInt32LE(audio.length - 8, 4);
  audio.write("WAVEfmt ", 8);
  audio.writeUInt32LE(16, 16);
  audio.writeUInt16LE(1, 20);
  audio.writeUInt16LE(1, 22);
  audio.writeUInt32LE(rate, 24);
  audio.writeUInt32LE(rate * 2, 28);
  audio.writeUInt16LE(2, 32);
  audio.writeUInt16LE(16, 34);
  audio.write("data", 36);
  audio.writeUInt32LE(samples * 2, 40);
  for (let index = 0; index < samples; index++)
    audio.writeInt16LE(
      Math.round(4000 * Math.sin((2 * Math.PI * frequency * index) / rate)),
      44 + index * 2,
    );
  return audio;
}

export async function nextEntryFixture() {
  if (
    process.env.HITO_NEXT_ENTRY_E2E !== "1" ||
    process.env.PLAYWRIGHT_NO_COPY_PROMPT !== "1"
  )
    throw new Error("The isolated next-entry privacy guard is required");
  const suffix = randomUUID().slice(0, 8);
  const token = `owned-next-entry-bearer-${suffix}`;
  const reference = `owned-next-entry-playlist-${suffix}`;
  const artist = `Owned next-entry performer ${suffix}`;
  const first = {
    id: "original/first",
    title: `First orbit ${suffix}`,
    artist,
    duration_ms: 20000,
    revision: "first-one",
  } satisfies Schema["ProviderRecording"];
  const next = {
    ...first,
    id: "original/next",
    title: `Second orbit ${suffix}`,
    duration_ms: 8000,
    revision: "next-one",
  } satisfies Schema["ProviderRecording"];
  const candidate = { ...next, id: "alternate/next" };
  const recordings = new Map(
    [first, next, candidate].map((recording) => [recording.id, recording]),
  );
  const firstSource = "private/source/first";
  const nextSource = "private/source/next";
  const audio = new Map([
    [firstSource, wav(first.duration_ms / 1000, 440)],
    [nextSource, wav(next.duration_ms / 1000, 660)],
  ]);
  const playlist = {
    id: "playlist/next-entry",
    name: `Owned next-entry playlist ${suffix}`,
    snapshot: "one",
    total_count: 2,
    attribution: { label: "Owned next-entry fixture" },
  };
  const counts = {
    search: 0,
    firstDetails: 0,
    firstSources: 0,
    firstAudio: 0,
    nextAudio: 0,
    protocol: 0,
    unexpected: 0,
    handler: 0,
  };
  let releaseSearch!: () => void;
  const searchGate = new Promise<void>((resolve) => {
    releaseSearch = resolve;
  });
  let searchReleased = false;
  let searchEntered = false;
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
      const input = await body(request);
      if (url.pathname === "/fixture/v1/audio") {
        const source = url.searchParams.get("source_id") ?? "";
        const bytes = audio.get(source);
        if (
          !["GET", "HEAD"].includes(request.method ?? "") ||
          input !== null ||
          url.searchParams.size !== 1 ||
          !bytes
        )
          throw new ProtocolFailure(404);
        let start = 0;
        let end = bytes.length - 1;
        const range = request.headers.range;
        if (range) {
          const match = /^bytes=(\d+)-(\d*)$/.exec(range);
          if (
            !match ||
            !Number.isSafeInteger(Number(match[1])) ||
            Number(match[1]) >= bytes.length ||
            (match[2] !== "" &&
              (!Number.isSafeInteger(Number(match[2])) ||
                Number(match[2]) < Number(match[1])))
          )
            throw new ProtocolFailure(416);
          start = Number(match[1]);
          if (match[2]) end = Math.min(end, Number(match[2]));
        }
        if (request.method === "GET")
          source === firstSource ? counts.firstAudio++ : counts.nextAudio++;
        response.writeHead(range ? 206 : 200, {
          "content-type": "audio/wav",
          "content-length": end - start + 1,
          "accept-ranges": "bytes",
          ...(range
            ? { "content-range": `bytes ${start}-${end}/${bytes.length}` }
            : {}),
        });
        response.end(
          request.method === "HEAD"
            ? undefined
            : bytes.subarray(start, end + 1),
        );
        return;
      }
      if (url.searchParams.size) throw new ProtocolFailure();
      let reply: unknown;
      if (url.pathname === "/fixture/v1/manifest") {
        if (request.method !== "GET" || input !== null)
          throw new ProtocolFailure();
        reply = {
          id: "org.hito.owned-next-entry-fixture",
          name: "Owned next-entry service",
          protocol_version: 1,
          capabilities: ["playlists", "search", "sources"],
        } satisfies Schema["Manifest"];
      } else {
        if (request.method !== "POST") throw new ProtocolFailure(405);
        switch (url.pathname) {
          case "/fixture/v1/playlists/detail": {
            const value = fields(
              input,
              input?.kind === "reference"
                ? ["kind", "reference"]
                : ["kind", "playlist_id"],
            );
            if (
              !(
                (value.kind === "reference" && value.reference === reference) ||
                (value.kind === "canonical" &&
                  value.playlist_id === playlist.id)
              )
            )
              throw new ProtocolFailure();
            reply = playlist;
            break;
          }
          case "/fixture/v1/playlists/items": {
            const value = fields(input, [
              "playlist_id",
              "snapshot",
              "cursor",
              "limit",
            ]);
            if (
              value.playlist_id !== playlist.id ||
              value.snapshot !== playlist.snapshot ||
              value.cursor !== null ||
              value.limit !== 100
            )
              throw new ProtocolFailure();
            reply = {
              playlist_id: playlist.id,
              snapshot: playlist.snapshot,
              items: [first, next].map((recording, index) => ({
                occurrence_id: `entry-${index}`,
                item: { kind: "recording", recording },
              })),
              next_cursor: null,
            };
            break;
          }
          case "/fixture/v1/search": {
            const value = fields(input, ["query", "cursor", "limit"]);
            if (
              typeof value.query !== "string" ||
              !value.query.includes(next.title) ||
              !value.query.includes(next.artist) ||
              value.query.includes(next.id) ||
              value.cursor !== null ||
              !Number.isInteger(value.limit) ||
              Number(value.limit) < 1 ||
              Number(value.limit) > 20
            )
              throw new ProtocolFailure();
            counts.search++;
            searchEntered = true;
            await searchGate;
            reply = { items: [candidate], next_cursor: null };
            break;
          }
          case "/fixture/v1/recordings/detail":
          case "/fixture/v1/sources": {
            const value = fields(input, ["recording_id"]);
            if (
              typeof value.recording_id !== "string" ||
              !recordings.has(value.recording_id)
            )
              throw new ProtocolFailure(404);
            const recording = recordings.get(value.recording_id)!;
            if (recording.id === first.id) {
              if (url.pathname.endsWith("/detail")) counts.firstDetails++;
              else counts.firstSources++;
            }
            reply = url.pathname.endsWith("/detail")
              ? recording
              : {
                  items:
                    recording.id === next.id
                      ? []
                      : [
                          {
                            id:
                              recording.id === first.id
                                ? firstSource
                                : nextSource,
                            label: "Owned playable WAV",
                            ready: true,
                          },
                        ],
                };
            break;
          }
          default:
            counts.unexpected++;
            response.writeHead(404).end();
            return;
        }
      }
      if (!response.destroyed)
        response
          .writeHead(200, { "content-type": "application/json" })
          .end(JSON.stringify(reply));
    } catch (error) {
      if (error instanceof ProtocolFailure) counts.protocol++;
      else counts.handler++;
      if (!response.destroyed) {
        if (!response.headersSent)
          response.writeHead(
            error instanceof ProtocolFailure ? error.status : 500,
          );
        response.end();
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
  return {
    address: `http://127.0.0.1:${(server.address() as AddressInfo).port}/fixture/`,
    token,
    reference,
    first,
    next,
    counts,
    privateValues: [
      token,
      reference,
      artist,
      first.title,
      next.title,
      first.id,
      next.id,
      candidate.id,
      firstSource,
      nextSource,
    ],
    get searchEntered() {
      return searchEntered;
    },
    releaseSearch: () => {
      searchReleased = true;
      releaseSearch();
    },
    matchesAudio: (
      kind: "first" | "next",
      bytes: Buffer,
      contentRange?: string,
    ) => {
      const expected = audio.get(kind === "first" ? firstSource : nextSource)!;
      if (!contentRange) return bytes.equals(expected);
      const match = /^bytes (\d+)-(\d+)\/(\d+)$/.exec(contentRange);
      return (
        match !== null &&
        Number(match[3]) === expected.length &&
        bytes.equals(expected.subarray(Number(match[1]), Number(match[2]) + 1))
      );
    },
    close: async () => {
      if (!searchReleased) releaseSearch();
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      );
    },
  };
}
