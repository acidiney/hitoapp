import { randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";
import { createServer, type ServerResponse } from "node:http";
import type { AddressInfo } from "node:net";
import { isDeepStrictEqual } from "node:util";
import type { components } from "../../src/generated";

type Schema = components["schemas"];

export async function artworkFixture() {
  const unique = randomUUID();
  const prefix = `Artwork ${unique.slice(0, 8)}`;
  const token = `artwork-private-token-${unique}`;
  const reference = `https://provider.invalid/private-artwork-playlist/${unique}`;
  const image = await readFile(
    new URL("./images/cover-a.png", import.meta.url),
  );
  const manifest: Schema["Manifest"] = {
    id: "org.hito.browser.artwork-fixture",
    name: `${prefix} provider`,
    protocol_version: 1,
    capabilities: ["playlists"],
  };
  const recording = {
    id: `private-recording/${unique}`,
    title: `${prefix} recording`,
    artist: "Hito owned artwork fixtures",
    duration_ms: 123000,
  } satisfies Schema["ProviderRecording"];
  const detail = {
    id: `private-playlist/${unique}`,
    name: `${prefix} copy`,
    snapshot: "owned-snapshot-one",
    total_count: 2,
    attribution: { label: `${prefix} owned attribution` },
  };
  const items = {
    playlist_id: detail.id,
    snapshot: detail.snapshot,
    items: ["first", "duplicate"].map((occurrence_id) => ({
      occurrence_id,
      item: { kind: "recording", recording },
    })),
    next_cursor: null,
  };
  const requests: {
    method: string | undefined;
    path: string;
    body: unknown;
    authorization: string | undefined;
    cookie: string | undefined;
    protocol: string | string[] | undefined;
  }[] = [];
  const held = new Set<ServerResponse>();
  let released = false;
  let offline = false;
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
    requests.push({
      method: request.method,
      path,
      body,
      authorization: request.headers.authorization,
      cookie: request.headers.cookie,
      protocol: request.headers["x-hito-plugin-protocol"],
    });
    if (offline) {
      response.writeHead(503).end();
      return;
    }
    if (
      path === "/fixture/v1/manifest" &&
      request.method === "GET" &&
      body === null
    ) {
      if (
        request.headers.authorization &&
        request.headers.authorization !== `Bearer ${token}`
      ) {
        response.writeHead(401).end();
        return;
      }
      response
        .writeHead(200, { "content-type": "application/json" })
        .end(JSON.stringify(manifest));
      return;
    }
    if (request.headers.authorization !== `Bearer ${token}`) {
      response.writeHead(401).end();
      return;
    }
    if (path === "/fixture/v1/artwork" && request.method === "POST") {
      if (
        !manifest.capabilities.includes("artwork") ||
        !isDeepStrictEqual(body, {
          entity: "recording",
          provider_id: recording.id,
        })
      ) {
        response.writeHead(400).end();
        return;
      }
      response.writeHead(200, {
        "content-type": "image/png",
        "content-length": image.length,
      });
      response.write(image.subarray(0, 8));
      if (released) response.end(image.subarray(8));
      else {
        held.add(response);
        response.once("close", () => held.delete(response));
      }
      return;
    }
    let reply: unknown;
    if (path === "/fixture/v1/playlists/detail" && request.method === "POST") {
      const expected = [
        { kind: "reference", reference },
        { kind: "canonical", playlist_id: detail.id },
      ];
      if (!expected.some((value) => isDeepStrictEqual(value, body))) {
        response.writeHead(400).end();
        return;
      }
      reply = detail;
    } else if (
      path === "/fixture/v1/playlists/items" &&
      request.method === "POST"
    ) {
      if (
        !isDeepStrictEqual(body, {
          playlist_id: detail.id,
          snapshot: detail.snapshot,
          cursor: null,
          limit: 100,
        })
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
    manifest,
    recording,
    detail,
    image,
    requests,
    get heldImages() {
      return held.size;
    },
    enableArtwork: () => {
      manifest.capabilities = ["playlists", "artwork"];
    },
    releaseImage: () => {
      released = true;
      for (const response of held) response.end(image.subarray(8));
      held.clear();
    },
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
