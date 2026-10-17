import { createServer, type IncomingMessage } from "node:http";
import type { AddressInfo } from "node:net";

const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const nil = "00000000-0000-0000-0000-000000000000";

function ownedWav() {
  const rate = 16_000;
  const samples = rate * 10;
  const audio = Buffer.alloc(44 + samples * 2);
  audio.write("RIFF", 0);
  audio.writeUInt32LE(audio.length - 8, 4);
  audio.write("WAVEfmt ", 8);
  audio.writeUInt32LE(16, 16);
  audio.writeUInt16LE(1, 20);
  audio.writeUInt16LE(1, 22);
  audio.writeUInt32LE(rate, 24);
  audio.writeUInt32LE(rate * 2, 28);
  audio.writeUInt16LE(2, 32);
  audio.writeUInt16LE(16, 34);
  audio.write("data", 36);
  audio.writeUInt32LE(samples * 2, 40);
  for (let sample = 0; sample < samples; sample++)
    audio.writeInt16LE(
      Math.round(Math.sin((sample * 440 * 2 * Math.PI) / rate) * 8192),
      44 + sample * 2,
    );
  return audio;
}

async function body(request: IncomingMessage) {
  const chunks: Buffer[] = [];
  let bytes = 0;
  for await (const chunk of request) {
    const value = Buffer.from(chunk);
    bytes += value.length;
    if (bytes > 32_768) throw new Error("Fixture request exceeds its limit");
    chunks.push(value);
  }
  const value: unknown = JSON.parse(Buffer.concat(chunks).toString("utf8"));
  if (value === null || typeof value !== "object" || Array.isArray(value))
    throw new Error("Fixture request must contain an object");
  return value as Record<string, unknown>;
}

export async function sourcePreparationFixture() {
  if (
    process.env.HITO_SOURCE_PREPARATION_E2E !== "1" ||
    process.env.PLAYWRIGHT_NO_COPY_PROMPT !== "1"
  )
    throw new Error("The isolated preparation privacy guard is required");
  const token = "owned-browser-preparation-service-token";
  const recording = {
    id: "private-preparation-recording",
    title: "Owned preparation recording",
    artist: "Hito test fixtures",
    duration_ms: 10_000,
  };
  const source = "private-preparation-exact-source";
  const handle = "private-preparation-acknowledged-handle";
  const label = "Owned exact audio";
  const audio = ownedWav();
  const counts = { begin: 0, status: 0, audio: 0, sources: 0, invalid: 0 };
  let operation: string | null = null;
  let releaseReady = false;
  let prepared = false;
  const preparing = {
    phase: "preparing",
    progress_basis_points: 2500,
    eta_seconds: 15,
    poll_after_seconds: 5,
  };
  const server = createServer(async (request, response) => {
    try {
      if (
        request.headers.authorization !== `Bearer ${token}` ||
        request.headers["x-hito-plugin-protocol"] !== "1" ||
        request.headers.cookie !== undefined
      ) {
        counts.invalid++;
        response.writeHead(401).end();
        return;
      }
      const url = new URL(request.url ?? "/", "http://fixture.invalid");
      if (url.pathname === "/fixture/v1/audio") {
        counts.audio++;
        if (
          !prepared ||
          !["GET", "HEAD"].includes(request.method ?? "") ||
          url.searchParams.size !== 1 ||
          url.searchParams.get("source_id") !== source
        ) {
          counts.invalid++;
          response.writeHead(404).end();
          return;
        }
        let first = 0;
        let last = audio.length - 1;
        const range = request.headers.range;
        if (range) {
          const match = /^bytes=(\d+)-(\d*)$/.exec(range);
          if (
            !match ||
            Number(match[1]) >= audio.length ||
            (match[2] !== "" && Number(match[2]) < Number(match[1]))
          ) {
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
      if (url.pathname === "/fixture/v1/manifest" && request.method === "GET") {
        response.writeHead(200, { "content-type": "application/json" }).end(
          JSON.stringify({
            id: "org.hito.browser-preparation-fixture",
            name: "Owned preparation service",
            protocol_version: 1,
            capabilities: ["sources", "source_preparation"],
          }),
        );
        return;
      }
      if (request.method !== "POST")
        throw new Error("Unexpected fixture method");
      const value = await body(request);
      let reply: unknown;
      if (
        ["/fixture/v1/recordings/detail", "/fixture/v1/sources"].includes(
          url.pathname,
        ) &&
        Object.keys(value).length === 1 &&
        value.recording_id === recording.id
      ) {
        if (url.pathname === "/fixture/v1/sources") {
          counts.sources++;
          reply = { items: [{ id: source, label, ready: prepared }] };
        } else reply = recording;
      } else {
        const poll = url.pathname === "/fixture/v1/sources/preparation/status";
        if (
          (!poll && url.pathname !== "/fixture/v1/sources/prepare") ||
          Object.keys(value).sort().join(",") !==
            (poll
              ? "handle,operation_id,recording_id,source_id"
              : "operation_id,recording_id,source_id") ||
          typeof value.operation_id !== "string" ||
          !uuid.test(value.operation_id) ||
          value.operation_id === nil ||
          value.recording_id !== recording.id ||
          value.source_id !== source ||
          (poll &&
            (value.operation_id !== operation || value.handle !== handle))
        )
          throw new Error("Unexpected fixture preparation identity");
        const target = {
          operation_id: value.operation_id,
          recording_id: value.recording_id,
          source_id: value.source_id,
        };
        if (poll) {
          counts.status++;
          if (releaseReady) prepared = true;
          reply = {
            ...target,
            handle,
            status: prepared ? { phase: "ready" } : preparing,
          };
        } else {
          counts.begin++;
          operation = value.operation_id;
          reply = {
            ...target,
            submission: { outcome: "accepted", handle, status: preparing },
          };
        }
      }
      response
        .writeHead(200, { "content-type": "application/json" })
        .end(JSON.stringify(reply));
    } catch {
      counts.invalid++;
      if (!response.headersSent) response.writeHead(500);
      response.end();
    }
  });
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", reject);
      resolve();
    });
  });
  return {
    address: `http://127.0.0.1:${(server.address() as AddressInfo).port}/fixture/`,
    token,
    recording,
    label,
    counts,
    privateValues: [token, recording.id, source, handle],
    operation: () => operation,
    releaseReady: () => {
      releaseReady = true;
    },
    close: async () => {
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      );
    },
  };
}
