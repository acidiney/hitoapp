import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import type { Schema } from "../../src/shared/schema";

export type GateAudioReply = {
  status: 200 | 206;
  start?: number;
  end?: number;
  contentRange?: string;
  prefixBytes?: number;
};

function ownedWav() {
  const rate = 16_000;
  const samples = rate * 90;
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
  return bytes;
}

export async function audiobookPlaybackGateFixture(
  reply?: (
    length: number,
    request: { ordinal: number; range: string | undefined },
  ) => GateAudioReply | undefined,
  beforeSyncReceipt?: (batch: Schema<"SyncBatch">) => Promise<void>,
) {
  const recordings = [
    "10000000-0000-4000-8000-000000000001",
    "10000000-0000-4000-8000-000000000002",
  ];
  const bytes = ownedWav();
  const sources = recordings.map((recording_id, index) => [
    {
      id: `20000000-0000-4000-8000-00000000000${index + 1}`,
      recording_id,
      provider: "local",
      device_id: null,
      ready: true,
      label: "Owned playback gate audio",
      can_prepare: false,
    } satisfies Schema<"Source">,
  ]);
  const book: Schema<"AudiobookDetail"> = {
    playlist: {
      id: "30000000-0000-4000-8000-000000000001",
      name: "Owned playback gate book",
      kind: "local",
    },
    chapters: [recordings[0], recordings[1], recordings[0]].map(
      (recording_id, position) => ({
        entry_id: `40000000-0000-4000-8000-00000000000${position + 1}`,
        recording_id,
        position,
        title: "Owned playback gate chapter",
        inherited_title: "Owned playback gate chapter",
        artist: "Owned playback gate artist",
        inherited_artist: "Owned playback gate artist",
        duration_ms: 90_000,
        available: true,
        plays_from_services: false,
        liked: false,
      }),
    ),
    bookmarks: [],
  };
  const target = {
    account: "50000000-0000-4000-8000-000000000001",
    book,
    recordings,
    sources,
    audio: sources.map(([source]) => ({
      path: `/api/v1/sources/${source.id}/audio`,
      bytes,
    })),
  };
  const counts = { requests: 0, sync: 0, errors: 0 };
  let audioRequests = 0;
  const server = createServer(async (request, response) => {
    try {
      if (++counts.requests > 64) throw new Error("Owned gate request budget");
      const path = new URL(request.url ?? "/", "http://owned.invalid").pathname;
      if (request.method === "GET" && path === "/") {
        response
          .writeHead(200, { "content-type": "text/html" })
          .end(
            '<!doctype html><html><head><link rel="icon" href="data:,"></head><body>' +
              '<button type="button" id="play">Play owned audio</button>' +
              '<audio data-hito-player preload="none"></audio><script>' +
              `document.querySelector('#play').onclick=()=>{const audio=document.querySelector('audio');audio.src=${JSON.stringify(target.audio[0].path)};void audio.play();};` +
              "</script></body></html>",
          );
        return;
      }
      if (
        request.method === "GET" &&
        target.audio.some((audio) => audio.path === path)
      ) {
        let value = reply?.(bytes.length, {
          ordinal: ++audioRequests,
          range: request.headers.range,
        });
        if (!value) {
          const range = request.headers.range?.match(/^bytes=(\d+)-(\d*)$/);
          if (range) {
            const start = Number(range[1]);
            const end = range[2] ? Number(range[2]) : bytes.length - 1;
            value = {
              status: 206,
              start,
              end,
              contentRange: `bytes ${start}-${end}/${bytes.length}`,
            };
          } else value = { status: 200 };
        }
        const body = bytes.subarray(
          value.start ?? 0,
          (value.end ?? bytes.length - 1) + 1,
        );
        if (
          value.prefixBytes !== undefined &&
          (!Number.isSafeInteger(value.prefixBytes) ||
            value.prefixBytes < 44 ||
            value.prefixBytes > 1_048_576 ||
            value.prefixBytes >= body.length)
        )
          throw new Error("Owned gate audio prefix bound");
        response.writeHead(value.status, {
          "content-type": "audio/wav",
          "content-length": body.length,
          "accept-ranges": "bytes",
          ...(value.contentRange
            ? { "content-range": value.contentRange }
            : {}),
        });
        if (value.prefixBytes !== undefined) {
          response.write(body.subarray(0, value.prefixBytes));
          await new Promise<void>((resolve) => response.once("close", resolve));
        } else response.end(body);
        return;
      }
      if (request.method === "POST" && path === "/api/v1/sync") {
        const chunks: Buffer[] = [];
        let size = 0;
        for await (const chunk of request) {
          size += chunk.length;
          if (size > 16_384) throw new Error("Owned gate request body bound");
          chunks.push(Buffer.from(chunk));
        }
        const batch = JSON.parse(
          Buffer.concat(chunks).toString(),
        ) as Schema<"SyncBatch">;
        counts.sync++;
        await beforeSyncReceipt?.(batch);
        response
          .writeHead(200, {
            "content-type": "application/json",
            "cache-control": "private, no-store",
          })
          .end(JSON.stringify({ acknowledged: [batch.operations[0].id] }));
        return;
      }
      throw new Error("Owned gate route unavailable");
    } catch {
      counts.errors++;
      response.writeHead(500).end();
    }
  });
  server.requestTimeout = 10_000;
  server.headersTimeout = 10_000;
  server.setTimeout(10_000, (socket) => socket.destroy());
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", reject);
      resolve();
    });
  });
  return {
    origin: `http://127.0.0.1:${(server.address() as AddressInfo).port}`,
    target,
    counts,
    async close() {
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      );
    },
  };
}
