import { randomUUID } from "node:crypto";
import {
  createServer,
  type IncomingMessage,
  type ServerResponse,
} from "node:http";
import type { AddressInfo } from "node:net";
import { isDeepStrictEqual } from "node:util";
import type { Schema } from "../../src/shared/schema";

export async function artistIdentityGroupsProvider(query: string) {
  if (process.env.HITO_ARTIST_IDENTITY_GROUPS_E2E !== "1")
    throw new Error("The owned artist group provider requires explicit opt-in");
  const token = `owned-private-artist-groups-${randomUUID()}`;
  const reference = `https://provider.invalid/owned-selection/${randomUUID()}`;
  const manifest = {
    id: "org.hito.browser.artist-identity-groups",
    name: `Owned artist groups ${randomUUID()}`,
    protocol_version: 1,
    capabilities: ["catalog", "playlists"],
  } satisfies Schema<"Manifest">;
  const candidates = ["First namesake", "Second namesake"].map(
    (disambiguation, index) => ({
      artist: {
        id: `opaque:artist/${index}?credit=shared~á`,
        name: "Owned namesake",
      },
      kind: "person" as const,
      disambiguation,
    }),
  ) satisfies Schema<"ArtistSearchCandidate">[];
  const profiles = candidates.map((candidate) => ({
    artist: candidate.artist,
    sort_name: null,
    kind: candidate.kind,
    disambiguation: `${candidate.disambiguation}, reviewed profile`,
    revision: "owned-profile:1",
  })) satisfies Schema<"CatalogArtistProfile">[];
  const recordings = [0, 1].map((index) => ({
    id: `opaque:recording/${index}`,
    title: `Owned imported recording ${index} ${randomUUID()}`,
    artist: candidates[index].artist.name,
    duration_ms: 90_000,
    release: {
      id: `opaque:release/${index}`,
      title: `Owned imported album ${index} ${randomUUID()}`,
    },
    disc_number: 1,
    track_number: 1,
    revision: "owned-recording:1",
  })) satisfies Schema<"ProviderRecording">[];
  const playlist = {
    id: "opaque:playlist/identity-groups",
    name: `Owned imported selection ${randomUUID()}`,
    snapshot: "owned-selection:1",
    total_count: 3,
    attribution: { label: "Owned artist group curator" },
  };
  const calls: string[] = [];
  const counters = { requests: 0, unexpected: 0, unsafeHeaders: 0 };
  const active = new Set<Promise<void>>();
  async function handle(request: IncomingMessage, response: ServerResponse) {
    const reject = () => {
      counters.unexpected++;
      response.writeHead(400).end();
    };
    if (++counters.requests > 100) return reject();
    const path = request.url ?? "";
    const manifestRequest = path === "/owned/v1/manifest";
    const authorization = request.headers.authorization;
    if (
      request.headers.cookie ||
      request.headers["x-hito-plugin-protocol"] !== "1" ||
      (authorization !== `Bearer ${token}` &&
        !(manifestRequest && !authorization))
    ) {
      counters.unsafeHeaders++;
      return reject();
    }
    const chunks: Buffer[] = [];
    let length = 0;
    for await (const chunk of request) {
      length += Buffer.byteLength(chunk);
      if (length > 4096) return reject();
      chunks.push(Buffer.from(chunk));
    }
    const bytes = Buffer.concat(chunks);
    const body: unknown = bytes.length ? JSON.parse(bytes.toString()) : null;
    const reply = (kind: string, value: unknown) => {
      calls.push(kind);
      response
        .writeHead(200, { "content-type": "application/json" })
        .end(JSON.stringify(value));
    };
    if (request.method === "GET" && body === null) {
      if (manifestRequest)
        return reply(
          authorization ? "manifest_private" : "manifest_public",
          manifest,
        );
      if (path === "/owned/v1/catalog/discovery/capabilities")
        return reply("discovery", {
          discovery_version: 1,
          operations: ["artist_search"],
        });
      if (path === "/owned/v1/catalog/capabilities")
        return reply("profile_descriptor", {
          catalog_version: 1,
          operations: ["artist_profile"],
        });
    }
    if (request.method === "POST") {
      if (
        path === "/owned/v1/playlists/detail" &&
        (isDeepStrictEqual(body, { kind: "reference", reference }) ||
          isDeepStrictEqual(body, {
            kind: "canonical",
            playlist_id: playlist.id,
          }))
      )
        return reply("playlist_detail", playlist);
      if (
        path === "/owned/v1/playlists/items" &&
        isDeepStrictEqual(body, {
          playlist_id: playlist.id,
          snapshot: playlist.snapshot,
          cursor: null,
          limit: 100,
        })
      )
        return reply("playlist_items", {
          playlist_id: playlist.id,
          snapshot: playlist.snapshot,
          next_cursor: null,
          items: [0, 1, 0].map((index, occurrence) => ({
            occurrence_id: `owned-occurrence/${occurrence}`,
            item: { kind: "recording", recording: recordings[index] },
          })),
        });
      if (
        path === "/owned/v1/catalog/artists/search" &&
        isDeepStrictEqual(body, {
          query,
          cursor: null,
          snapshot: null,
          limit: 20,
        })
      )
        return reply("search", {
          query,
          items: candidates,
          total_count: 2,
          next_cursor: null,
          snapshot: "owned-search:1",
        });
      const index = profiles.findIndex((profile) =>
        isDeepStrictEqual(body, { artist_id: profile.artist.id }),
      );
      if (path === "/owned/v1/catalog/artists/profile" && index >= 0)
        return reply(`profile_${index}`, profiles[index]);
    }
    reject();
  }
  const server = createServer((request, response) => {
    const task = handle(request, response)
      .catch(() => {
        counters.unexpected++;
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
    token,
    reference,
    manifest,
    candidates,
    profiles,
    recordings,
    playlist,
    calls,
    counters,
    address: `${origin}/owned/`,
    origin,
    query,
    privateValues: [
      token,
      reference,
      manifest.id,
      manifest.name,
      playlist.id,
      playlist.name,
      ...candidates.flatMap((candidate) => [
        candidate.artist.id,
        candidate.artist.name,
        candidate.disambiguation,
      ]),
      ...profiles.flatMap((profile) => [
        profile.revision,
        profile.disambiguation,
      ]),
      ...recordings.flatMap((recording) => [
        recording.id,
        recording.title,
        recording.release.id,
        recording.release.title,
      ]),
    ],
    async close() {
      let timer: ReturnType<typeof setTimeout> | undefined;
      try {
        await Promise.race([
          new Promise<void>((resolve, reject) =>
            server.close((error) =>
              error
                ? reject(new Error("Owned artist provider close failed"))
                : resolve(),
            ),
          ),
          new Promise<never>((_resolve, reject) => {
            timer = setTimeout(
              () => reject(new Error("Owned artist provider close timed out")),
              3000,
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
