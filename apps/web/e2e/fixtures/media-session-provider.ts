import { randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";
import { createServer, type IncomingMessage } from "node:http";
import type { AddressInfo } from "node:net";
import type { components } from "../../src/generated";

type Schema = components["schemas"];
class ProtocolError extends Error {}

async function object(request: IncomingMessage) {
  const chunks: Buffer[] = [];
  let bytes = 0;
  for await (const chunk of request) {
    const value = Buffer.from(chunk);
    bytes += value.length;
    if (bytes > 8192) throw new ProtocolError();
    chunks.push(value);
  }
  if (!bytes) return null;
  const value: unknown = JSON.parse(Buffer.concat(chunks).toString("utf8"));
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new ProtocolError();
  return value as Record<string, unknown>;
}

function wav(frequency: number) {
  const rate = 16000;
  const samples = rate * 60;
  const bytes = Buffer.alloc(44 + samples * 2);
  bytes.write("RIFF", 0);
  bytes.writeUInt32LE(bytes.length - 8, 4);
  bytes.write("WAVEfmt ", 8);
  bytes.writeUInt32LE(16, 16);
  bytes.writeUInt16LE(1, 20);
  bytes.writeUInt16LE(1, 22);
  bytes.writeUInt32LE(rate, 24);
  bytes.writeUInt32LE(rate * 2, 28);
  bytes.writeUInt16LE(2, 32);
  bytes.writeUInt16LE(16, 34);
  bytes.write("data", 36);
  bytes.writeUInt32LE(samples * 2, 40);
  for (let index = 0; index < samples; index++)
    bytes.writeInt16LE(
      Math.round(3000 * Math.sin((2 * Math.PI * frequency * index) / rate)),
      44 + index * 2,
    );
  return bytes;
}

export async function mediaSessionProvider() {
  if (
    process.env.HITO_MEDIA_SESSION_E2E !== "1" ||
    process.env.PLAYWRIGHT_NO_COPY_PROMPT !== "1"
  )
    throw new Error("The isolated media session privacy guard is required");
  const suffix = randomUUID();
  const token = `owned-media-session-private-bearer-${suffix}`;
  const artist = `Owned private media-session performer ${suffix}`;
  const tracks: Schema["ProviderRecording"][] = ["first", "second"].map(
    (part) => ({
      id: `private/media-session/${part}/${suffix}`,
      title: `Owned private ${part} title ${suffix}`,
      artist,
      duration_ms: 60000,
      revision: "one",
    }),
  );
  const sources = tracks.map((track, index) => ({
    recording: track.id,
    id: `private/media-session/source/${index}/${suffix}`,
    bytes: wav(index ? 660 : 440),
  }));
  const cover = await readFile(
    new URL(
      "../../../../crates/hito-core/tests/fixtures/cover-a.png",
      import.meta.url,
    ),
  );
  const width = cover.readUInt32BE(16);
  const height = cover.readUInt32BE(20);
  const counts = {
    audio: 0,
    artwork: 0,
    details: 0,
    sources: 0,
    protocol: 0,
    unexpected: 0,
    handler: 0,
  };
  const server = createServer(async (request, response) => {
    response.on("error", () => counts.handler++);
    try {
      const url = new URL(request.url ?? "/", "http://fixture.invalid");
      const input = await object(request);
      const manifest = url.pathname === "/fixture/v1/manifest";
      if (
        request.headers.cookie !== undefined ||
        request.headers["x-hito-plugin-protocol"] !== "1" ||
        (request.headers.authorization !== `Bearer ${token}` &&
          !(manifest && request.headers.authorization === undefined))
      )
        throw new ProtocolError();
      if (url.pathname === "/fixture/v1/audio") {
        const source = sources.find(
          (source) => source.id === url.searchParams.get("source_id"),
        );
        if (
          !source ||
          input !== null ||
          url.searchParams.size !== 1 ||
          !["GET", "HEAD"].includes(request.method ?? "")
        )
          throw new ProtocolError();
        let first = 0;
        let last = source.bytes.length - 1;
        const range = request.headers.range;
        if (range) {
          const match = /^bytes=(\d+)-(\d*)$/.exec(range);
          if (
            !match ||
            !Number.isSafeInteger(Number(match[1])) ||
            Number(match[1]) >= source.bytes.length ||
            (match[2] !== "" &&
              (!Number.isSafeInteger(Number(match[2])) ||
                Number(match[2]) < Number(match[1])))
          )
            throw new ProtocolError();
          first = Number(match[1]);
          if (match[2]) last = Math.min(last, Number(match[2]));
        }
        if (request.method === "GET") counts.audio++;
        response.writeHead(range ? 206 : 200, {
          "content-type": "audio/wav",
          "content-length": last - first + 1,
          "accept-ranges": "bytes",
          ...(range
            ? {
                "content-range": `bytes ${first}-${last}/${source.bytes.length}`,
              }
            : {}),
        });
        response.end(
          request.method === "HEAD"
            ? undefined
            : source.bytes.subarray(first, last + 1),
        );
        return;
      }
      if (url.searchParams.size) throw new ProtocolError();
      let reply: unknown;
      if (manifest) {
        if (request.method !== "GET" || input !== null)
          throw new ProtocolError();
        reply = {
          id: "org.hito.owned-media-session",
          name: "Owned media session service",
          protocol_version: 1,
          capabilities: ["sources", "artwork"],
        } satisfies Schema["Manifest"];
      } else {
        if (request.method !== "POST" || !input) throw new ProtocolError();
        if (url.pathname === "/fixture/v1/artwork") {
          if (
            Object.keys(input).sort().join() !== "entity,provider_id" ||
            input.entity !== "recording" ||
            !tracks.some((track) => track.id === input.provider_id)
          )
            throw new ProtocolError();
          counts.artwork++;
          response
            .writeHead(200, {
              "content-type": "image/png",
              "content-length": cover.length,
            })
            .end(cover);
          return;
        }
        if (Object.keys(input).join() !== "recording_id")
          throw new ProtocolError();
        const track = tracks.find((track) => track.id === input.recording_id);
        if (!track) throw new ProtocolError();
        if (url.pathname === "/fixture/v1/recordings/detail") {
          counts.details++;
          reply = track;
        } else if (url.pathname === "/fixture/v1/sources") {
          counts.sources++;
          reply = {
            items: [
              {
                id: sources.find((source) => source.recording === track.id)!.id,
                label: "Owned WAV",
                ready: true,
              },
            ],
          };
        } else {
          counts.unexpected++;
          throw new ProtocolError();
        }
      }
      response
        .writeHead(200, { "content-type": "application/json" })
        .end(JSON.stringify(reply));
    } catch (error) {
      if (error instanceof ProtocolError || error instanceof SyntaxError)
        counts.protocol++;
      else counts.handler++;
      if (!response.destroyed) {
        if (!response.headersSent) response.writeHead(400);
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
    tracks,
    cover,
    width,
    height,
    counts,
    privateValues: [
      token,
      artist,
      ...tracks.flatMap((track) => [track.id, track.title]),
      ...sources.map((source) => source.id),
    ],
    close: async () => {
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      );
    },
  };
}
