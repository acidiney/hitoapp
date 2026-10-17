import { randomUUID } from "node:crypto";
import {
  createServer,
  type IncomingMessage,
  type ServerResponse,
} from "node:http";
import type { AddressInfo } from "node:net";
import type { components } from "../../src/generated";

type Schema = components["schemas"];
export type Binding = "a" | "b";
type Role = "metadata" | Binding;
const roles: Role[] = ["metadata", "a", "b"];

class ProtocolFailure extends Error {
  constructor(readonly status = 400) {
    super("Owned manual source protocol rejected");
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

function exact(value: Record<string, unknown> | null, keys: string[]) {
  if (
    !value ||
    Object.keys(value).sort().join(",") !== [...keys].sort().join(",")
  )
    throw new ProtocolFailure();
  return value;
}

function wav(frequency: number) {
  const rate = 16000;
  const samples = rate * 12;
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
      Math.round(4000 * Math.sin((2 * Math.PI * frequency * index) / rate)),
      44 + index * 2,
    );
  return bytes;
}

function counters() {
  return {
    requests: 0,
    manifests: 0,
    playlists: 0,
    searches: 0,
    details: 0,
    metadata: 0,
    sources: 0,
    audioGet: 0,
    audioHead: 0,
    audioOutages: 0,
    ranges: 0,
  };
}

export async function manualSourceChoiceFixture() {
  if (
    process.env.HITO_AUTOMATIC_SOURCE_E2E !== "1" ||
    process.env.PLAYWRIGHT_NO_COPY_PROMPT !== "1"
  )
    throw new Error("The isolated manual source privacy guard is required");
  const suffix = randomUUID().slice(0, 8);
  const title = `Owned orbit ${suffix}`;
  const artist = "Owned studio performer";
  const original = {
    id: "metadata/original",
    title,
    artist,
    duration_ms: 12000,
    release: { id: "metadata/studio", title: "Studio sessions", artist },
    disc_number: 1,
    track_number: 1,
    revision: "metadata-one",
  } satisfies Schema["ProviderRecording"];
  const recordings = {
    metadata: original,
    a: {
      ...original,
      id: "a/studio",
      release: { ...original.release, id: "a/release" },
      revision: "a-one",
    },
    b: {
      ...original,
      id: "b/studio",
      release: { ...original.release, id: "b/release" },
      revision: "b-one",
    },
  } satisfies Record<Role, Schema["ProviderRecording"]>;
  const sources = { a: "audio/a/studio", b: "audio/b/studio" };
  const labels = { a: "Owned studio A", b: "Owned studio B" };
  const bytes = { a: wav(440), b: wav(660) };
  const ready = { a: true, b: true };
  const outage = { a: false, b: false };
  let bSearchable = false;
  const queries: { role: Binding; query: string }[] = [];
  const counts = {
    metadata: counters(),
    a: counters(),
    b: counters(),
    protocol: 0,
    unexpected: 0,
    handler: 0,
  };
  const token = `owned-manual-source-${suffix}`;
  const reference = `owned-manual-playlist-${suffix}`;
  const playlist = {
    id: "playlist/duplicates",
    name: `Owned manual choices ${suffix}`,
    snapshot: "one",
    total_count: 2,
    attribution: { label: "Owned playlist metadata" },
  };
  const handlers = new Set<Promise<void>>();
  let closing = false;

  async function exchange(request: IncomingMessage, response: ServerResponse) {
    response.on("error", () => {
      if (!closing) counts.handler++;
    });
    try {
      const url = new URL(request.url ?? "/", "http://fixture.invalid");
      const parts = /^\/(metadata|a|b)\/v1\/(.+)$/.exec(url.pathname);
      if (!parts) {
        counts.unexpected++;
        throw new ProtocolFailure(404);
      }
      const role = parts[1] as Role;
      const path = parts[2];
      const count = counts[role];
      count.requests++;
      if (
        request.headers.authorization !== `Bearer ${token}` ||
        request.headers["x-hito-plugin-protocol"] !== "1" ||
        request.headers.cookie !== undefined
      )
        throw new ProtocolFailure(401);
      const body = await readBody(request);
      if (path === "audio") {
        if (
          role === "metadata" ||
          !["GET", "HEAD"].includes(request.method ?? "") ||
          body !== null ||
          url.searchParams.size !== 1 ||
          url.searchParams.get("source_id") !== sources[role]
        )
          throw new ProtocolFailure(404);
        if (request.method === "GET") count.audioGet++;
        else count.audioHead++;
        if (outage[role]) {
          count.audioOutages++;
          response.writeHead(503).end();
          return;
        }
        const audio = bytes[role];
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
          count.ranges++;
        }
        response
          .writeHead(range ? 206 : 200, {
            "content-type": "audio/wav",
            "content-length": last - first + 1,
            "accept-ranges": "bytes",
            ...(range
              ? { "content-range": `bytes ${first}-${last}/${audio.length}` }
              : {}),
          })
          .end(
            request.method === "HEAD"
              ? undefined
              : audio.subarray(first, last + 1),
          );
        return;
      }
      if (url.searchParams.size !== 0 || request.headers.range !== undefined)
        throw new ProtocolFailure();
      let reply: unknown;
      if (path === "manifest") {
        if (request.method !== "GET" || body !== null)
          throw new ProtocolFailure();
        count.manifests++;
        reply = {
          id: `org.hito.manual-choice-${role}`,
          name: `Owned ${role} service`,
          protocol_version: 1,
          capabilities:
            role === "metadata"
              ? ["playlists", "metadata"]
              : ["search", "sources", "metadata"],
        } satisfies Schema["Manifest"];
      } else {
        if (request.method !== "POST") throw new ProtocolFailure(405);
        if (role === "metadata" && path === "playlists/detail") {
          const input = exact(
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
          count.playlists++;
          reply = playlist;
        } else if (role === "metadata" && path === "playlists/items") {
          const input = exact(body, [
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
          count.playlists++;
          reply = {
            playlist_id: playlist.id,
            snapshot: playlist.snapshot,
            items: ["first", "duplicate"].map((occurrence_id) => ({
              occurrence_id,
              item: { kind: "recording", recording: original },
            })),
            next_cursor: null,
          };
        } else if (role !== "metadata" && path === "search") {
          const input = exact(body, ["query", "cursor", "limit"]);
          if (
            typeof input.query !== "string" ||
            !input.query.trim() ||
            Buffer.byteLength(input.query) > 512 ||
            /[\u0000-\u001f\u007f]/.test(input.query) ||
            input.cursor !== null ||
            !Number.isInteger(input.limit) ||
            typeof input.limit !== "number" ||
            input.limit < 1 ||
            input.limit > 20
          )
            throw new ProtocolFailure();
          count.searches++;
          queries.push({ role, query: input.query });
          reply = {
            items: role === "b" && !bSearchable ? [] : [recordings[role]],
            next_cursor: null,
          } satisfies Schema["SearchPage"];
        } else if (
          ["recordings/detail", "recordings/metadata", "sources"].includes(path)
        ) {
          const input = exact(body, ["recording_id"]);
          if (input.recording_id !== recordings[role].id)
            throw new ProtocolFailure(404);
          if (path === "recordings/detail") {
            count.details++;
            reply = recordings[role];
          } else if (path === "recordings/metadata") {
            count.metadata++;
            reply = {
              recording: recordings[role],
              musical_title: title,
              album_title: original.release.title,
              source_ids: role === "metadata" ? [] : [sources[role]],
            };
          } else {
            if (role === "metadata") throw new ProtocolFailure(404);
            count.sources++;
            reply = {
              items: [
                { id: sources[role], label: labels[role], ready: ready[role] },
              ],
            };
          }
        } else {
          counts.unexpected++;
          throw new ProtocolFailure(404);
        }
      }
      response
        .writeHead(200, { "content-type": "application/json" })
        .end(JSON.stringify(reply));
    } catch (error) {
      if (closing && request.destroyed) return;
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
        if (!closing) counts.handler++;
      }
    }
  }

  const server = createServer((request, response) => {
    const handling = exchange(request, response)
      .catch(() => {
        counts.handler++;
      })
      .finally(() => handlers.delete(handling));
    handlers.add(handling);
  });
  server.requestTimeout = 5000;
  server.headersTimeout = 5000;
  server.maxHeadersCount = 32;
  server.on("clientError", (_error, socket) => {
    if (!closing) counts.protocol++;
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
    if (!closing) counts.handler++;
  });
  const address = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  return {
    addresses: Object.fromEntries(
      roles.map((role) => [role, `${address}/${role}/`]),
    ) as Record<Role, string>,
    token,
    reference,
    playlist,
    original,
    recordings,
    labels,
    counts,
    audio: (binding: Binding) => bytes[binding],
    showB: () => {
      bSearchable = true;
    },
    setReady: (binding: Binding, value: boolean) => {
      ready[binding] = value;
    },
    setAudioUnavailable: (binding: Binding, value: boolean) => {
      outage[binding] = value;
    },
    snapshot: () => JSON.stringify(counts),
    activeCallbacks: () => handlers.size,
    canonicalAutomaticSearches: (excluded: string[]) =>
      queries.some((item) => item.role === "a") &&
      queries
        .filter((item) => item.role === "a")
        .every(
          ({ query }) =>
            query === `${artist} ${title}` &&
            excluded.every((value) => !query.includes(value)),
        ),
    privateValues: [
      token,
      reference,
      playlist.name,
      title,
      artist,
      address,
      ...roles.flatMap((role) => [recordings[role].id, `${address}/${role}/`]),
      ...Object.values(sources),
    ],
    close: async () => {
      closing = true;
      let timeout: ReturnType<typeof setTimeout> | undefined;
      try {
        const stopped = new Promise<void>((resolve, reject) =>
          server.close((error) =>
            error
              ? reject(new Error("Owned provider shutdown failed"))
              : resolve(),
          ),
        );
        server.closeAllConnections();
        await Promise.race([
          (async () => {
            await stopped;
            while (handlers.size) await Promise.allSettled([...handlers]);
          })(),
          new Promise<never>((_resolve, reject) => {
            timeout = setTimeout(
              () => reject(new Error("Owned provider shutdown timed out")),
              5000,
            );
          }),
        ]);
        if (handlers.size)
          throw new Error("Owned provider callbacks must settle");
      } finally {
        clearTimeout(timeout);
      }
    },
  };
}
