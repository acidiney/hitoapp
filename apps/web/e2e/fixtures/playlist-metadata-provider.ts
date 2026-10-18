import { randomUUID } from "node:crypto";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { isDeepStrictEqual } from "node:util";
import type { components } from "../../src/generated";

type Schema = components["schemas"];

export async function playlistMetadataProvider() {
  if (process.env.HITO_PLAYLIST_METADATA_E2E !== "1")
    throw new Error("The disposable metadata fixture requires explicit opt-in");
  const prefix = `Owned metadata ${randomUUID().slice(0, 8)}`;
  const token = `private-metadata-token-${randomUUID()}`;
  const reference = `https://provider.invalid/metadata/${randomUUID()}`;
  const manifest = {
    id: "org.hito.browser.playlist-metadata",
    name: `${prefix} provider`,
    protocol_version: 1,
    capabilities: ["playlists"],
  } satisfies Schema["Manifest"];
  const album = {
    id: "release/verified",
    title: `${prefix} album`,
  } satisfies Schema["ProviderRelease"];
  const recordings = [
    {
      id: "recording/one",
      title: `${prefix} first`,
      artist: `${prefix} performer one`,
      duration_ms: 123_000,
      revision: "albumless-one",
    },
    {
      id: "recording/two",
      title: `${prefix} second`,
      artist: `${prefix} performer two`,
      duration_ms: 234_000,
      revision: "albumless-one",
    },
  ] satisfies Schema["ProviderRecording"][];
  const verified = recordings.map((recording, index) => ({
    ...recording,
    title: `${recording.title} verified`,
    revision: "verified-one",
    release: album,
    disc_number: index + 1,
    track_number: index === 0 ? 2 : 1,
  }));
  const playlist = {
    id: "playlist/metadata",
    name: `${prefix} selection`,
    snapshot: "albumless-one",
    total_count: 5,
    attribution: { label: `${prefix} curator` },
  };
  const entries = [
    {
      occurrence_id: "second-first",
      item: { kind: "recording", recording: recordings[1] },
    },
    {
      occurrence_id: "first-next",
      item: { kind: "recording", recording: recordings[0] },
    },
    {
      occurrence_id: "second-again",
      item: { kind: "recording", recording: recordings[1] },
    },
    {
      occurrence_id: "retained-unavailable",
      item: {
        kind: "unavailable",
        provider_recording_id: "recording/withdrawn",
        title: `${prefix} withdrawn`,
        artist: `${prefix} retained performer`,
      },
    },
    { occurrence_id: "retained-unknown", item: { kind: "unknown" } },
  ];
  let enriched = false;
  let offline = false;
  const calls = {
    manifest: 0,
    detail: 0,
    items: 0,
    offline: 0,
    unexpected: 0,
    unsafe_headers: 0,
    total: 0,
  };
  const server = createServer(async (request, response) => {
    try {
      if (++calls.total > 32) {
        calls.unexpected++;
        response.writeHead(429).end();
        return;
      }
      if (
        request.headers.cookie ||
        (request.headers.authorization !== undefined &&
          request.headers.authorization !== `Bearer ${token}`)
      ) {
        calls.unsafe_headers++;
        response.writeHead(401).end();
        return;
      }
      const target = new URL(request.url ?? "/", "http://fixture.invalid");
      const chunks: Buffer[] = [];
      let bytes = 0;
      for await (const chunk of request) {
        bytes += chunk.length;
        if (bytes > 8192) {
          calls.unexpected++;
          response.writeHead(413).end();
          return;
        }
        chunks.push(Buffer.from(chunk));
      }
      const body: unknown = bytes
        ? JSON.parse(Buffer.concat(chunks).toString())
        : null;
      const detail = enriched
        ? {
            ...playlist,
            name: `${prefix} upstream changed`,
            snapshot: "verified-one",
            total_count: 3,
          }
        : playlist;
      let reply: unknown;
      if (target.search || target.hash) {
        calls.unexpected++;
        response.writeHead(400).end();
        return;
      }
      if (
        target.pathname === "/fixture/v1/manifest" &&
        request.method === "GET" &&
        body === null
      ) {
        calls.manifest++;
        reply = manifest;
      } else if (request.headers.authorization !== `Bearer ${token}`) {
        calls.unsafe_headers++;
        response.writeHead(401).end();
        return;
      } else if (
        target.pathname === "/fixture/v1/playlists/detail" &&
        request.method === "POST" &&
        (isDeepStrictEqual(body, { kind: "reference", reference }) ||
          isDeepStrictEqual(body, {
            kind: "canonical",
            playlist_id: playlist.id,
          }))
      ) {
        if (
          enriched &&
          !isDeepStrictEqual(body, {
            kind: "canonical",
            playlist_id: playlist.id,
          })
        ) {
          calls.unexpected++;
          response.writeHead(400).end();
          return;
        }
        calls.detail++;
        reply = detail;
      } else if (
        target.pathname === "/fixture/v1/playlists/items" &&
        request.method === "POST" &&
        isDeepStrictEqual(body, {
          playlist_id: playlist.id,
          snapshot: detail.snapshot,
          cursor: null,
          limit: 100,
        })
      ) {
        calls.items++;
        reply = {
          playlist_id: playlist.id,
          snapshot: detail.snapshot,
          next_cursor: null,
          items: enriched
            ? [
                {
                  occurrence_id: "new-upstream",
                  item: {
                    kind: "recording",
                    recording: {
                      id: "recording/new-upstream",
                      title: `${prefix} unrelated`,
                      artist: `${prefix} unrelated performer`,
                    },
                  },
                },
                {
                  occurrence_id: "first-next",
                  item: { kind: "recording", recording: verified[0] },
                },
                {
                  occurrence_id: "second-first",
                  item: { kind: "recording", recording: verified[1] },
                },
              ]
            : entries,
        };
      } else {
        calls.unexpected++;
        response.writeHead(404).end();
        return;
      }
      if (offline) {
        calls.offline++;
        response.writeHead(503).end();
        return;
      }
      response
        .writeHead(200, { "content-type": "application/json" })
        .end(JSON.stringify(reply));
    } catch {
      calls.unexpected++;
      response.writeHead(500).end();
    }
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
  return {
    prefix,
    token,
    reference,
    manifest,
    album,
    recordings,
    verified,
    playlist,
    calls,
    address: `http://127.0.0.1:${(server.address() as AddressInfo).port}/fixture/`,
    enrich: () => {
      enriched = true;
    },
    offline: () => {
      offline = true;
    },
    close: async () => {
      await new Promise<void>((resolve, reject) => {
        const timer = setTimeout(
          () => reject(new Error("Owned metadata provider close timed out")),
          3000,
        );
        server.close((error) => {
          clearTimeout(timer);
          if (error) reject(new Error("Owned metadata provider close failed"));
          else resolve();
        });
        server.closeAllConnections();
      });
    },
  };
}
