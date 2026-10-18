import { randomUUID } from "node:crypto";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { isDeepStrictEqual } from "node:util";
import type { Page } from "@playwright/test";
import type { components } from "../../src/generated";

type Schema = components["schemas"];

export async function releaseCreditsFixture() {
  const prefix = `Owned credit ${randomUUID().slice(0, 8)}`;
  const token = `private-credit-token-${randomUUID()}`;
  const reference = `https://provider.invalid/credit/${randomUUID()}`;
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
      release: album,
      disc_number: 1,
      track_number: 2,
      revision: "metadata-one",
    },
    {
      id: "recording/two",
      title: `${prefix} second`,
      artist: `${prefix} performer two`,
      duration_ms: 234_000,
      release: album,
      disc_number: 2,
      track_number: 1,
      revision: "metadata-one",
    },
  ] satisfies Schema["ProviderRecording"][];
  const manifest = {
    id: "org.hito.browser.release-credits",
    name: `${prefix} provider`,
    protocol_version: 1,
    capabilities: ["playlists"],
  } satisfies Schema["Manifest"];
  const playlist = {
    id: "playlist/credits",
    name: `${prefix} selection`,
    snapshot: "credits-one",
    total_count: 3,
    attribution: {
      label: `${prefix} curator`,
    } satisfies Schema["PlaylistAttribution"],
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
  ];
  const page = {
    playlist_id: playlist.id,
    snapshot: playlist.snapshot,
    items: entries,
    next_cursor: null,
  };
  const calls = {
    manifest: 0,
    detail: 0,
    items: 0,
    unexpected: 0,
    unsafe_headers: 0,
    total: 0,
  };
  const server = createServer(async (request, response) => {
    try {
      if (++calls.total > 32) {
        response.writeHead(429).end();
        return;
      }
      const target = new URL(request.url ?? "/", "http://fixture.invalid");
      if (
        request.headers.cookie ||
        (request.headers.authorization !== undefined &&
          request.headers.authorization !== `Bearer ${token}`)
      ) {
        calls.unsafe_headers++;
        response.writeHead(401).end();
        return;
      }
      const chunks: Buffer[] = [];
      let bytes = 0;
      for await (const chunk of request) {
        bytes += chunk.length;
        if (bytes > 8192) {
          response.writeHead(413).end();
          return;
        }
        chunks.push(Buffer.from(chunk));
      }
      const body: unknown = bytes
        ? JSON.parse(Buffer.concat(chunks).toString())
        : null;
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
        isDeepStrictEqual(body, { kind: "reference", reference })
      ) {
        calls.detail++;
        reply = playlist;
      } else if (
        target.pathname === "/fixture/v1/playlists/items" &&
        request.method === "POST" &&
        isDeepStrictEqual(body, {
          playlist_id: playlist.id,
          snapshot: playlist.snapshot,
          cursor: null,
          limit: 100,
        })
      ) {
        calls.items++;
        reply = page;
      } else {
        calls.unexpected++;
        response.writeHead(404).end();
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
    album,
    recordings,
    manifest,
    playlist,
    calls,
    address: `http://127.0.0.1:${(server.address() as AddressInfo).port}/fixture/`,
    close: async () => {
      await new Promise<void>((resolve, reject) => {
        const timer = setTimeout(
          () => reject(new Error("Owned provider close timed out")),
          3000,
        );
        server.close((error) => {
          clearTimeout(timer);
          if (error) reject(new Error("Owned provider close failed"));
          else resolve();
        });
        server.closeAllConnections();
      });
    },
  };
}

export function releaseCreditReader(page: Page) {
  let reads = 0;
  return async <T>(path: string): Promise<T> => {
    if (++reads > 64) throw new Error("Public read budget exceeded");
    const response = await page.request.get(path, { timeout: 10_000 });
    if (response.status() !== 200) throw new Error("Public read failed");
    const body = await response.body();
    if (body.length > 524_288)
      throw new Error("Public read exceeded its bound");
    return JSON.parse(body.toString()) as T;
  };
}

export async function creditPost<T>(
  page: Page,
  path: string,
  status: number,
  click: () => Promise<void>,
) {
  const [response] = await Promise.all([
    page.waitForResponse(
      (candidate) =>
        new URL(candidate.url()).pathname === path &&
        candidate.request().method() === "POST",
      { timeout: 15_000 },
    ),
    click(),
  ]);
  if (response.status() !== status) throw new Error("Public action failed");
  const body = await response.body();
  if (body.length > 524_288)
    throw new Error("Public action exceeded its bound");
  return {
    body: JSON.parse(body.toString()) as T,
    sent: response.request().postDataJSON() as unknown,
  };
}

export async function creditAction(
  page: Page,
  expected: Schema["Action"],
  click: () => Promise<void>,
) {
  const result = await creditPost<Schema["SyncReceipt"]>(
    page,
    "/api/v1/sync",
    200,
    click,
  );
  const batch = result.sent as Schema["SyncBatch"];
  if (
    batch.operations.length !== 1 ||
    !isDeepStrictEqual(batch.operations[0].action, expected) ||
    !/^[a-f0-9-]{36}$/.test(batch.operations[0].id) ||
    !result.body.acknowledged.includes(batch.operations[0].id)
  )
    throw new Error("The public mutation receipt did not match its action");
  return batch.operations[0].id;
}
