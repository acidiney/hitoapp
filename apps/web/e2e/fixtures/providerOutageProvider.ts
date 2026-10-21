import { randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";
import { createServer, type IncomingMessage } from "node:http";
import type { AddressInfo } from "node:net";
import { isDeepStrictEqual } from "node:util";
import type { components } from "../../src/generated";

type Schema = components["schemas"];
export type OutageTrack = "retry" | "queue" | "healthy" | "preserved";
type Availability = "empty" | "unavailable" | "ready";

export async function providerOutageProvider() {
  if (process.env.HITO_PROVIDER_OUTAGE_E2E !== "1")
    throw new Error("The owned provider requires explicit opt-in");
  const token = `owned-outage-${randomUUID()}`;
  const suffix = randomUUID();
  const audio = await readFile(
    "/workspace/fixtures/e2e-generated/Music/Fixture 440 Hz.wav",
  );
  if (
    audio.length > 4 * 1024 * 1024 ||
    audio.subarray(0, 4).toString() !== "RIFF"
  )
    throw new Error("The owned WAV fixture is invalid");
  const keys: OutageTrack[] = ["retry", "queue", "healthy", "preserved"];
  const recordings = Object.fromEntries(
    keys.map((key) => [
      key,
      {
        id: `opaque:outage/${key}/${suffix}`,
        title: `Owned ${key} recording ${suffix}`,
        artist: `Owned outage performer ${suffix}`,
        duration_ms: 90_000,
        release: {
          id: `opaque:release/${key}/${suffix}`,
          title: `Owned ${key} release`,
        },
        revision: "owned-outage:1",
      } satisfies Schema["ProviderRecording"],
    ]),
  ) as Record<OutageTrack, Schema["ProviderRecording"]>;
  const sourceRecordings = Object.fromEntries(
    keys.map((key) => [
      key,
      { ...recordings[key], id: `opaque:outage-source/${key}/${suffix}` },
    ]),
  ) as Record<OutageTrack, Schema["ProviderRecording"]>;
  const availability: Record<OutageTrack, Availability> = {
    retry: "empty",
    queue: "unavailable",
    healthy: "ready",
    preserved: "ready",
  };
  const selections = {
    retry: ["retry"],
    queue: ["healthy", "queue", "healthy"],
    preserved: ["preserved", "preserved"],
  } satisfies Record<string, OutageTrack[]>;
  const playlists = Object.fromEntries(
    Object.entries(selections).map(([key, items]) => [
      key,
      {
        id: `opaque:playlist/${key}/${suffix}`,
        name: `Owned ${key} playlist ${suffix}`,
        snapshot: "owned:1",
        total_count: items.length,
        attribution: { label: "Owned fixture" },
      },
    ]),
  );
  const manifest = {
    id: "org.hito.browser.provider-outage",
    name: `Owned outage service ${suffix}`,
    protocol_version: 1,
    capabilities: ["search", "metadata", "sources", "playlists"],
    content_policy: { source_persistence: "playback_only" },
  } satisfies Schema["Manifest"];
  const counts = {
    requests: 0,
    search: 0,
    detail: 0,
    metadata: 0,
    sources: 0,
    unavailable: 0,
    audioGet: 0,
    audioHead: 0,
    ranges: 0,
    rejected: 0,
  };
  const sourceCounts = Object.fromEntries(
    keys.map((key) => [key, 0]),
  ) as Record<OutageTrack, number>;
  const active = new Set<Promise<void>>();
  async function body(request: IncomingMessage) {
    const chunks: Buffer[] = [];
    let length = 0;
    for await (const chunk of request) {
      length += Buffer.byteLength(chunk);
      if (length > 16_384) throw new Error("Owned request exceeded its bound");
      chunks.push(Buffer.from(chunk));
    }
    return length
      ? (JSON.parse(Buffer.concat(chunks).toString()) as unknown)
      : null;
  }
  const exact = (value: unknown, expected: object) =>
    isDeepStrictEqual(value, expected);
  const server = createServer((request, response) => {
    const task = (async () => {
      const url = new URL(request.url ?? "/", "http://owned.invalid");
      const path = url.pathname;
      const declaration = path === "/owned/v1/manifest";
      if (
        ++counts.requests > 256 ||
        request.headers.cookie ||
        request.headers["x-hito-plugin-protocol"] !== "1" ||
        (request.headers.authorization !== `Bearer ${token}` &&
          !(declaration && !request.headers.authorization))
      )
        throw new Error("Owned request envelope rejected");
      const input = await body(request);
      const reply = (value: unknown): void => {
        response
          .writeHead(200, { "content-type": "application/json" })
          .end(JSON.stringify(value));
      };
      if (
        declaration &&
        request.method === "GET" &&
        input === null &&
        !url.search
      )
        return reply(manifest);
      if (path === "/owned/v1/audio") {
        const key = keys.find(
          (key) => url.searchParams.get("source_id") === `owned:audio/${key}`,
        );
        if (
          !key ||
          !["GET", "HEAD"].includes(request.method ?? "") ||
          input !== null ||
          url.searchParams.size !== 1
        )
          throw new Error("Owned audio envelope rejected");
        if (request.method === "GET") counts.audioGet++;
        else counts.audioHead++;
        let first = 0;
        let last = audio.length - 1;
        const range = request.headers.range;
        if (range) {
          const match = /^bytes=(\d+)-(\d*)$/.exec(range);
          if (
            !match ||
            !Number.isSafeInteger(Number(match[1])) ||
            Number(match[1]) >= audio.length ||
            (match[2] &&
              (!Number.isSafeInteger(Number(match[2])) ||
                Number(match[2]) < Number(match[1])))
          )
            throw new Error("Owned audio range rejected");
          first = Number(match[1]);
          if (match[2]) last = Math.min(last, Number(match[2]));
          counts.ranges++;
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
      if (request.method !== "POST" || url.search || request.headers.range)
        throw new Error("Owned provider method rejected");
      for (const [key, playlist] of Object.entries(playlists)) {
        if (
          path === "/owned/v1/playlists/detail" &&
          (exact(input, { kind: "reference", reference: key }) ||
            exact(input, { kind: "canonical", playlist_id: playlist.id }))
        )
          return reply(playlist);
        if (
          path === "/owned/v1/playlists/items" &&
          exact(input, {
            playlist_id: playlist.id,
            snapshot: playlist.snapshot,
            cursor: null,
            limit: 100,
          })
        )
          return reply({
            playlist_id: playlist.id,
            snapshot: playlist.snapshot,
            next_cursor: null,
            items: selections[key as keyof typeof selections].map(
              (track, index) => ({
                occurrence_id: `owned:entry/${index}`,
                item: { kind: "recording", recording: recordings[track] },
              }),
            ),
          });
      }
      const value = input as Record<string, unknown> | null;
      if (
        path === "/owned/v1/search" &&
        value &&
        Object.keys(value).sort().join() === "cursor,limit,query" &&
        value.cursor === null &&
        Number.isInteger(value.limit) &&
        Number(value.limit) >= 1 &&
        Number(value.limit) <= 20 &&
        typeof value.query === "string" &&
        Buffer.byteLength(value.query) <= 512
      ) {
        counts.search++;
        return reply({
          items: keys
            .filter(
              (key) =>
                value.query ===
                `${sourceRecordings[key].artist} ${sourceRecordings[key].title}`,
            )
            .map((key) => sourceRecordings[key]),
          next_cursor: null,
        } satisfies Schema["SearchPage"]);
      }
      const key = keys.find((key) =>
        exact(input, { recording_id: sourceRecordings[key].id }),
      );
      if (!key) throw new Error("Owned recording identity rejected");
      if (path === "/owned/v1/recordings/detail") {
        counts.detail++;
        return reply(sourceRecordings[key]);
      }
      if (path === "/owned/v1/recordings/metadata") {
        counts.metadata++;
        return reply({
          recording: sourceRecordings[key],
          musical_title: sourceRecordings[key].title,
          album_title: sourceRecordings[key].release?.title,
          source_ids:
            availability[key] === "ready" ? [`owned:audio/${key}`] : [],
        });
      }
      if (path === "/owned/v1/sources") {
        counts.sources++;
        sourceCounts[key]++;
        if (availability[key] === "unavailable") {
          counts.unavailable++;
          response.writeHead(503).end();
          return;
        }
        return reply({
          items:
            availability[key] === "ready"
              ? [
                  {
                    id: `owned:audio/${key}`,
                    label: "Owned verified audio",
                    ready: true,
                  },
                ]
              : [],
        });
      }
      throw new Error("Owned provider route rejected");
    })()
      .catch(() => {
        counts.rejected++;
        response.destroy();
      })
      .finally(() => active.delete(task));
    active.add(task);
  });
  server.requestTimeout = 5_000;
  server.headersTimeout = 5_000;
  server.setTimeout(5_000, (socket) => socket.destroy());
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", reject);
      resolve();
    });
  });
  const address = `http://127.0.0.1:${(server.address() as AddressInfo).port}/owned/`;
  return {
    token,
    address,
    manifest,
    recordings,
    playlists,
    counts,
    sourceCounts,
    setAvailability(key: OutageTrack, value: Availability) {
      availability[key] = value;
    },
    privateValues: [
      token,
      address,
      manifest.id,
      manifest.name,
      ...keys.map((key) => `owned:audio/${key}`),
      ...Object.values(recordings).flatMap((item) => [
        item.id,
        item.title,
        item.artist,
        item.release!.id,
        item.release!.title,
      ]),
      ...Object.values(sourceRecordings).map((item) => item.id),
      ...Object.values(playlists).flatMap((item) => [item.id, item.name]),
    ],
    async close() {
      let timer: ReturnType<typeof setTimeout> | undefined;
      try {
        const stopped = new Promise<void>((resolve, reject) =>
          server.close((error) =>
            error
              ? reject(new Error("Owned provider close failed"))
              : resolve(),
          ),
        );
        server.closeAllConnections();
        await Promise.race([
          stopped,
          new Promise<never>((_resolve, reject) => {
            timer = setTimeout(
              () => reject(new Error("Owned provider close timed out")),
              5_000,
            );
          }),
        ]);
      } finally {
        clearTimeout(timer);
        server.closeAllConnections();
        await Promise.allSettled([...active]);
      }
    },
  };
}

export type OutageProvider = Awaited<ReturnType<typeof providerOutageProvider>>;
