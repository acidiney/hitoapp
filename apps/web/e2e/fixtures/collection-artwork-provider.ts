import { randomUUID } from "node:crypto";
import { readFile, stat } from "node:fs/promises";
import { createServer, type ServerResponse } from "node:http";
import type { AddressInfo } from "node:net";
import { isDeepStrictEqual } from "node:util";
import type { components } from "../../src/generated";

type Schema = components["schemas"];
export type Entity = "recording" | "release" | "playlist";
const images = {
  recording: "crates/core/tests/fixtures/cover-a.png",
  release: "crates/core/tests/fixtures/cover-b.png",
  playlist: "crates/hito-plugin-spotify/src/tests/fixtures/artwork.png",
} satisfies Record<Entity, string>;

export async function ownedOriginals() {
  return Promise.all(
    [
      "/owned-music/owned-original.wav",
      ...Object.values(images).map((path) => `/workspace/${path}`),
    ].map(async (path) => {
      const [bytes, metadata] = await Promise.all([readFile(path), stat(path)]);
      return {
        bytes,
        size: metadata.size,
        mode: metadata.mode,
        mtime: metadata.mtimeMs,
      };
    }),
  );
}

export async function collectionArtworkProvider() {
  const unique = randomUUID();
  const prefix = `Owned artwork ${unique.slice(0, 8)}`;
  const token = `private-artwork-bearer-${unique}`;
  const covers = Object.fromEntries(
    await Promise.all(
      Object.entries(images).map(async ([entity, path]) => [
        entity,
        await readFile(`/workspace/${path}`),
      ]),
    ),
  ) as Record<Entity, Buffer>;
  const manifest: Schema["Manifest"] = {
    id: "org.hito.collection-artwork-browser",
    name: `${prefix} provider`,
    protocol_version: 1,
    capabilities: ["playlists"],
  };
  const selections = ["base", "revoked", "logout"].map((label) => ({
    reference: `https://private-provider.invalid/${unique}/${label}?private=reference`,
    detail: {
      id: `private-playlist/${unique}/${label}`,
      name: `${prefix} ${label} selection`,
      snapshot: `private-snapshot-${label}`,
      total_count: 2,
      attribution: { label: `${prefix} attribution` },
    },
    recording: {
      id: `private-recording/${unique}/${label}`,
      title: `${prefix} ${label} track`,
      artist: `${prefix} performer`,
      duration_ms: 123000,
      revision: "owned-revision-one",
      release: {
        id: `private-release/${unique}/${label}`,
        title: `${prefix} ${label} album`,
      },
    } satisfies Schema["ProviderRecording"],
  }));
  const requests: {
    method: string | undefined;
    path: string;
    body: unknown;
    authorization: string | undefined;
    cookie: string | undefined;
    protocol: string | string[] | undefined;
  }[] = [];
  const gates = new Map<
    string,
    { responses: Set<ServerResponse>; entered: number; released: boolean }
  >();
  let offline = false;
  let violations = 0;
  const server = createServer(async (request, response) => {
    let raw = "";
    for await (const chunk of request) {
      raw += chunk;
      if (Buffer.byteLength(raw) > 8192) {
        violations++;
        response.writeHead(413).end();
        return;
      }
    }
    let body: unknown = null;
    try {
      if (raw) body = JSON.parse(raw);
    } catch {
      violations++;
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
    if (requests.length > 200) {
      violations++;
      response.writeHead(429).end();
      return;
    }
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
        violations++;
        response.writeHead(401).end();
        return;
      }
      response
        .writeHead(200, { "content-type": "application/json" })
        .end(JSON.stringify(manifest));
      return;
    }
    if (
      request.headers.authorization !== `Bearer ${token}` ||
      request.headers.cookie ||
      request.headers["x-hito-plugin-protocol"] !== "1"
    ) {
      violations++;
      response.writeHead(401).end();
      return;
    }
    if (request.method !== "POST") {
      violations++;
      response.writeHead(405).end();
      return;
    }
    if (path === "/fixture/v1/artwork") {
      const match = Object.keys(covers).find((entity) =>
        selections.some((selection) =>
          isDeepStrictEqual(body, {
            entity,
            provider_id:
              entity === "playlist"
                ? selection.detail.id
                : entity === "release"
                  ? selection.recording.release!.id
                  : selection.recording.id,
          }),
        ),
      ) as Entity | undefined;
      if (!manifest.capabilities.includes("artwork") || !match) {
        violations++;
        response.writeHead(400).end();
        return;
      }
      const image = covers[match];
      const gate = gates.get(JSON.stringify(body));
      response.writeHead(200, {
        "content-type": "image/png",
        "content-length": image.length,
      });
      response.write(image.subarray(0, 8));
      if (!gate || gate.released) response.end(image.subarray(8));
      else {
        gate.entered++;
        gate.responses.add(response);
        response.once("close", () => gate.responses.delete(response));
      }
      return;
    }
    const selection = selections.find((selection) =>
      path === "/fixture/v1/playlists/detail"
        ? [
            { kind: "reference", reference: selection.reference },
            { kind: "canonical", playlist_id: selection.detail.id },
          ].some((expected) => isDeepStrictEqual(body, expected))
        : path === "/fixture/v1/playlists/items" &&
          isDeepStrictEqual(body, {
            playlist_id: selection.detail.id,
            snapshot: selection.detail.snapshot,
            cursor: null,
            limit: 100,
          }),
    );
    if (!selection) {
      violations++;
      response.writeHead(404).end();
      return;
    }
    const reply = path.endsWith("/detail")
      ? selection.detail
      : {
          playlist_id: selection.detail.id,
          snapshot: selection.detail.snapshot,
          next_cursor: null,
          items: ["first", "duplicate"].map((occurrence_id) => ({
            occurrence_id,
            item: { kind: "recording", recording: selection.recording },
          })),
        };
    response
      .writeHead(200, { "content-type": "application/json" })
      .end(JSON.stringify(reply));
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = `http://127.0.0.1:${(server.address() as AddressInfo).port}/fixture/`;
  return {
    address,
    prefix,
    token,
    manifest,
    selections,
    covers,
    requests,
    get violations() {
      return violations;
    },
    enableArtwork: () => {
      manifest.capabilities = ["playlists", "artwork"];
    },
    offline: () => {
      offline = true;
    },
    hold: (entity: Entity, provider_id: string) => {
      const key = JSON.stringify({ entity, provider_id });
      const gate = {
        responses: new Set<ServerResponse>(),
        entered: 0,
        released: false,
      };
      gates.set(key, gate);
      return {
        get entered() {
          return gate.entered;
        },
        release: () => {
          gate.released = true;
          for (const response of gate.responses)
            response.end(covers[entity].subarray(8));
          gate.responses.clear();
        },
      };
    },
    privateValues: [
      token,
      address,
      ...selections.flatMap((selection) => [
        selection.reference,
        selection.detail.id,
        selection.detail.name,
        selection.detail.snapshot,
        selection.detail.attribution.label,
        selection.recording.id,
        selection.recording.title,
        selection.recording.artist,
        selection.recording.release!.id,
        selection.recording.release!.title,
      ]),
    ],
    close: async () => {
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      );
    },
  };
}

export type ArtworkProviderFixture = Awaited<
  ReturnType<typeof collectionArtworkProvider>
>;
