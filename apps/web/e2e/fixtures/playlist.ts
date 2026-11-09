import { randomUUID } from "node:crypto";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import type { Schema } from "../../src/shared/schema";

export async function playlistFixture() {
  const unique = randomUUID();
  const prefix = `Playlist ${unique.slice(0, 8)}`;
  const token = `playlist-private-bearer-${unique}`;
  const reference = `https://provider.invalid/private-playlist/${unique}`;
  const secondReference = `${reference}/another-copy`;
  const manifest = {
    id: "org.hito.browser.playlist-fixture",
    name: `${prefix} provider`,
    protocol_version: 1,
    capabilities: ["playlists"],
  } satisfies Schema<"Manifest">;
  const recording = {
    id: "recording/known",
    title: `${prefix} recording`,
    artist: "Hito playlist fixtures",
    duration_ms: 123000,
  } satisfies Schema<"ProviderRecording">;
  const detail = {
    id: "playlist/canonical",
    name: `${prefix} copy`,
    snapshot: "snapshot-one",
    total_count: 4,
    attribution: {
      label: `${prefix} curated attribution`,
    } satisfies Schema<"PlaylistAttribution">,
  };
  const withdrawn = `${prefix} withdrawn`;
  const items = {
    playlist_id: detail.id,
    snapshot: detail.snapshot,
    items: [
      { occurrence_id: "first", item: { kind: "recording", recording } },
      {
        occurrence_id: "withdrawn",
        item: {
          kind: "unavailable",
          provider_recording_id: "recording/withdrawn",
          title: withdrawn,
          artist: "Retained fixture artist",
        },
      },
      { occurrence_id: "unknown", item: { kind: "unknown" } },
      { occurrence_id: "duplicate", item: { kind: "recording", recording } },
    ],
    next_cursor: null,
  };
  let offline = false;
  const requests: {
    method: string | undefined;
    path: string;
    body: unknown;
    authorization: string | undefined;
    cookie: string | undefined;
  }[] = [];
  const server = createServer(async (request, response) => {
    let raw = "";
    for await (const chunk of request) {
      raw += chunk;
      if (Buffer.byteLength(raw) > 8192) {
        response.writeHead(413).end();
        return;
      }
    }
    let body: unknown = null;
    try {
      if (raw) body = JSON.parse(raw);
    } catch {
      response.writeHead(400).end();
      return;
    }
    const path = new URL(request.url ?? "/", "http://fixture.invalid").pathname;
    if (path.endsWith("/v1/health")) {
      response.writeHead(404).end();
      return;
    }
    requests.push({
      method: request.method,
      path,
      body,
      authorization: request.headers.authorization,
      cookie: request.headers.cookie,
    });
    if (request.headers.authorization !== `Bearer ${token}`) {
      response.writeHead(401).end();
      return;
    }
    if (offline) {
      response.writeHead(503).end();
      return;
    }
    let reply: unknown;
    if (
      path === "/fixture/v1/manifest" &&
      request.method === "GET" &&
      body === null
    )
      reply = manifest;
    else if (
      path === "/fixture/v1/playlists/detail" &&
      request.method === "POST"
    ) {
      if (
        !body ||
        typeof body !== "object" ||
        Object.keys(body).length !== 2 ||
        !("kind" in body) ||
        !(
          (body.kind === "reference" &&
            "reference" in body &&
            (body.reference === reference ||
              body.reference === secondReference)) ||
          (body.kind === "canonical" &&
            "playlist_id" in body &&
            body.playlist_id === detail.id)
        )
      ) {
        response.writeHead(400).end();
        return;
      }
      reply = detail;
    } else if (
      path === "/fixture/v1/playlists/items" &&
      request.method === "POST"
    ) {
      if (
        !body ||
        typeof body !== "object" ||
        Object.keys(body).length !== 4 ||
        !("playlist_id" in body) ||
        body.playlist_id !== detail.id ||
        !("snapshot" in body) ||
        body.snapshot !== detail.snapshot ||
        !("cursor" in body) ||
        body.cursor !== null ||
        !("limit" in body) ||
        body.limit !== 100
      ) {
        response.writeHead(400).end();
        return;
      }
      reply = items;
    } else {
      response.writeHead(404).end();
      return;
    }
    response
      .writeHead(200, { "content-type": "application/json" })
      .end(JSON.stringify(reply));
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  return {
    address: `http://127.0.0.1:${(server.address() as AddressInfo).port}/fixture/`,
    prefix,
    token,
    reference,
    secondReference,
    manifest,
    recording,
    detail,
    withdrawn,
    requests,
    offline: () => {
      offline = true;
    },
    close: async () => {
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      );
    },
  };
}
