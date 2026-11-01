import { randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { deflateSync } from "node:zlib";
import type { Schema } from "../../src/shared/schema";

function crc32(bytes: Buffer): number {
  let crc = 0xffffffff;
  for (const byte of bytes) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit++)
      crc = crc & 1 ? (crc >>> 1) ^ 0xedb88320 : crc >>> 1;
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function chunk(type: string, data: Buffer): Buffer {
  const body = Buffer.concat([Buffer.from(type), data]);
  const frame = Buffer.alloc(4);
  frame.writeUInt32BE(data.length);
  const check = Buffer.alloc(4);
  check.writeUInt32BE(crc32(body));
  return Buffer.concat([frame, body, check]);
}

function gradientPng(size: number): Buffer {
  const rows: Buffer[] = [];
  for (let y = 0; y < size; y++) {
    const row = Buffer.alloc(1 + size * 3);
    for (let x = 0; x < size; x++) {
      row[1 + x * 3] = 30 + x * 2;
      row[2 + x * 3] = 140;
      row[3 + x * 3] = 160 + ((y * 2) % 90);
    }
    rows.push(row);
  }
  const header = Buffer.alloc(13);
  header.writeUInt32BE(size, 0);
  header.writeUInt32BE(size, 4);
  header.set([8, 2, 0, 0, 0], 8);
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", header),
    chunk("IDAT", deflateSync(Buffer.concat(rows))),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

export async function featuredShelfFixture() {
  const unique = randomUUID().slice(0, 8);
  const prefix = `Featured ${unique}`;
  const token = `featured-private-bearer-${unique}`;
  const audio = await readFile(
    "/workspace/fixtures/e2e-generated/Music/Fixture 440 Hz.wav",
  );
  const cover = gradientPng(64);
  const manifest = {
    id: "org.hito.browser.featured-fixture",
    name: `${prefix} service`,
    protocol_version: 1,
    capabilities: ["search", "playlists", "sources", "featured", "artwork"],
  } satisfies Schema<"Manifest">;
  const items = [
    {
      id: "mix",
      title: `${prefix} morning mix`,
      subtitle: "Owned fixture curator",
      kind: "playlist",
      reference: "fixture-playlist-mix",
      artwork_id: "cover-mix",
      item_count: 2,
    },
    {
      id: "fresh",
      title: `${prefix} fresh album`,
      subtitle: null,
      kind: "new_release",
      reference: "fixture-album-fresh",
      artwork_id: null,
      item_count: 2,
    },
  ];
  const recordings = (reference: string) =>
    ["one", "two"].map((name) => ({
      id: `${reference}/${name}`,
      title: `${prefix} ${reference.slice(8)} ${name}`,
      artist: "Hito featured fixtures",
      duration_ms: 90_000,
    }));
  const requests: {
    path: string;
    body: unknown;
    authorization: string | undefined;
    cookie: string | undefined;
  }[] = [];
  const server = createServer(async (request, response) => {
    let raw = "";
    for await (const part of request) raw += part;
    const url = new URL(request.url ?? "/", "http://fixture.invalid");
    const body = raw ? (JSON.parse(raw) as unknown) : null;
    requests.push({
      path: url.pathname,
      body,
      authorization: request.headers.authorization,
      cookie: request.headers.cookie,
    });
    if (request.headers.authorization !== `Bearer ${token}`) {
      response.writeHead(401).end();
      return;
    }
    const json = (value: unknown) =>
      response
        .writeHead(200, { "content-type": "application/json" })
        .end(JSON.stringify(value));
    const asked = body as Record<string, string> | null;
    switch (url.pathname) {
      case "/fixture/v1/manifest":
        return json(manifest);
      case "/fixture/v1/featured":
        return json({ items });
      case "/fixture/v1/artwork":
        response
          .writeHead(200, { "content-type": "image/png" })
          .end(asked?.provider_id === "cover-mix" ? cover : undefined);
        return;
      case "/fixture/v1/playlists/detail": {
        const reference =
          asked?.kind === "reference" ? asked.reference : asked?.playlist_id;
        const item = items.find(
          (entry) =>
            entry.reference === reference ||
            `playlist/${entry.reference}` === reference,
        );
        if (!item) return void response.writeHead(404).end();
        return json({
          id: `playlist/${item.reference}`,
          name: item.title,
          snapshot: "snapshot-one",
          total_count: 2,
          attribution: { label: `${prefix} curated` },
        });
      }
      case "/fixture/v1/playlists/items": {
        const reference = String(asked?.playlist_id).slice("playlist/".length);
        return json({
          playlist_id: asked?.playlist_id,
          snapshot: "snapshot-one",
          items: recordings(reference).map((recording) => ({
            occurrence_id: null,
            item: { kind: "recording", recording },
          })),
          next_cursor: null,
        });
      }
      case "/fixture/v1/search":
        return json({
          items: items.flatMap((entry) => recordings(entry.reference)),
          next_cursor: null,
        });
      case "/fixture/v1/recordings/detail": {
        const found = items
          .flatMap((entry) => recordings(entry.reference))
          .find((recording) => recording.id === asked?.recording_id);
        if (!found) return void response.writeHead(404).end();
        return json(found);
      }
      case "/fixture/v1/sources":
        return json({
          items: [
            {
              id: `source:${asked?.recording_id}`,
              label: "Original",
              ready: true,
            },
          ],
        });
      case "/fixture/v1/audio": {
        let first = 0;
        let last = audio.length - 1;
        const range =
          request.method === "HEAD" ? undefined : request.headers.range;
        if (range) {
          const match = /^bytes=(\d+)-(\d*)$/.exec(range);
          if (!match || Number(match[1]) >= audio.length) {
            response
              .writeHead(416, { "content-range": `bytes */${audio.length}` })
              .end();
            return;
          }
          first = Number(match[1]);
          if (match[2]) last = Math.min(last, Number(match[2]));
        }
        response.writeHead(range ? 206 : 200, {
          "content-type": "audio/wav",
          "accept-ranges": "bytes",
          "content-length": last - first + 1,
          ...(range
            ? { "content-range": `bytes ${first}-${last}/${audio.length}` }
            : {}),
        });
        response.end(
          request.method === "HEAD"
            ? undefined
            : audio.subarray(first, last + 1),
        );
        return;
      }
      default:
        response.writeHead(404).end();
    }
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  return {
    address: `http://127.0.0.1:${(server.address() as AddressInfo).port}/fixture/`,
    prefix,
    token,
    items,
    requests,
    featuredRequests: () =>
      requests.filter((request) => request.path === "/fixture/v1/featured"),
    close: async () => {
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      );
    },
  };
}
