import { createServer } from "node:http";
import { createReadStream } from "node:fs";
import { readFile, stat } from "node:fs/promises";
import { createInterface } from "node:readline";
import { performance } from "node:perf_hooks";

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const epochMs = () => performance.timeOrigin + performance.now();

export async function startProvider({
  audioFile,
  recordings = 0,
  latencyMs = 0,
  pace = { burstBytes: 24000, bytesPerSecond: 16000 },
  token = "bench-provider-token",
  titlePrefix = "Bench Remote",
}) {
  const size = (await stat(audioFile)).size;
  const records = Array.from({ length: recordings }, (_, index) => ({
    id: `bench-remote-${index}`,
    title: `${titlePrefix} ${String(index).padStart(5, "0")}`,
    artist: "Bench Remote Artist",
    duration_ms: 60000,
  }));
  const byId = new Map(records.map((record) => [record.id, record]));
  const sourceLists = new Map();
  const failed = new Set();
  const pacedSources = new Set();
  const streams = new Map();
  const counters = { sources: 0, audio: 0, detail: 0 };
  let latency = latencyMs;

  const defaultSources = (id) => [
    { id: `audio/${id}/a`, label: "Primary", ready: true },
  ];

  async function respondAudio(request, response, sourceId) {
    counters.audio++;
    if (failed.has(sourceId)) {
      response.writeHead(503).end();
      return;
    }
    let first = 0;
    let last = size - 1;
    const range = request.headers.range;
    if (range) {
      const match = /^bytes=(\d+)-(\d*)$/.exec(range);
      if (!match || Number(match[1]) >= size) {
        response.writeHead(416, { "content-range": `bytes */${size}` }).end();
        return;
      }
      first = Number(match[1]);
      if (match[2]) last = Math.min(last, Number(match[2]));
    }
    response.writeHead(range ? 206 : 200, {
      "content-type": "audio/mpeg",
      "accept-ranges": "bytes",
      "content-length": last - first + 1,
      ...(range ? { "content-range": `bytes ${first}-${last}/${size}` } : {}),
    });
    if (request.method === "HEAD") {
      response.end();
      return;
    }
    if (!pacedSources.has(sourceId)) {
      createReadStream(audioFile, { start: first, end: last }).pipe(response);
      return;
    }
    const set = streams.get(sourceId) ?? new Set();
    streams.set(sourceId, set);
    set.add(response);
    response.once("close", () => set.delete(response));
    const body = await readFile(audioFile);
    let offset = first;
    const burst = Math.min(last + 1, offset + pace.burstBytes);
    response.write(body.subarray(offset, burst));
    offset = burst;
    const step = Math.max(1, Math.floor(pace.bytesPerSecond / 20));
    while (offset <= last && !response.destroyed) {
      await sleep(50);
      const end = Math.min(last + 1, offset + step);
      response.write(body.subarray(offset, end));
      offset = end;
    }
    if (!response.destroyed) response.end();
  }

  const server = createServer(async (request, response) => {
    const url = new URL(request.url, "http://provider.invalid");
    if (request.headers.authorization !== `Bearer ${token}`) {
      response.writeHead(401).end();
      return;
    }
    if (latency > 0) await sleep(latency);
    if (url.pathname === "/bench/v1/audio") {
      await respondAudio(
        request,
        response,
        url.searchParams.get("source_id") ?? "",
      );
      return;
    }
    let body = "";
    for await (const chunk of request) body += chunk;
    const input = body ? JSON.parse(body) : {};
    let reply;
    switch (url.pathname) {
      case "/bench/v1/manifest":
        reply = {
          id: "org.hito.bench-provider",
          name: "Bench provider",
          protocol_version: 1,
          capabilities: ["search", "sources"],
        };
        break;
      case "/bench/v1/search":
        reply = { items: records.slice(0, 20) };
        break;
      case "/bench/v1/recordings/detail":
        counters.detail++;
        reply = byId.get(input.recording_id) ?? null;
        break;
      case "/bench/v1/sources":
        counters.sources++;
        reply = {
          items:
            sourceLists.get(input.recording_id) ??
            defaultSources(input.recording_id),
        };
        break;
      default:
        response.writeHead(404).end();
        return;
    }
    response
      .writeHead(200, { "content-type": "application/json" })
      .end(JSON.stringify(reply));
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const port = server.address().port;

  return {
    address: `http://127.0.0.1:${port}/bench/`,
    token,
    records,
    counters,
    setLatency: (ms) => {
      latency = ms;
    },
    setSources: (recordingId, items) => sourceLists.set(recordingId, items),
    pace: (sourceId) => pacedSources.add(sourceId),
    unpace: (sourceId) => pacedSources.delete(sourceId),
    heal: (sourceId) => failed.delete(sourceId),
    streamsOpen: (sourceId) => streams.get(sourceId)?.size ?? 0,
    drop(sourceId) {
      failed.add(sourceId);
      const set = streams.get(sourceId) ?? new Set();
      for (const response of set) response.socket?.destroy();
      return epochMs();
    },
    close: async () => {
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    },
  };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const arguments_ = new Map();
  for (let index = 2; index < process.argv.length; index += 2)
    arguments_.set(process.argv[index], process.argv[index + 1]);
  const provider = await startProvider({
    audioFile: arguments_.get("--audio"),
    recordings: Number(arguments_.get("--recordings") ?? 0),
    latencyMs: Number(arguments_.get("--latency") ?? 0),
  });
  const emit = (value) => process.stdout.write(`${JSON.stringify(value)}\n`);
  emit({
    event: "listening",
    address: provider.address,
    token: provider.token,
  });
  const lines = createInterface({ input: process.stdin });
  lines.on("line", async (line) => {
    const command = JSON.parse(line);
    switch (command.cmd) {
      case "latency":
        provider.setLatency(command.ms);
        emit({ event: "ack", cmd: "latency" });
        break;
      case "pace":
        provider.pace(command.source);
        emit({ event: "ack", cmd: "pace" });
        break;
      case "unpace":
        provider.unpace(command.source);
        emit({ event: "ack", cmd: "unpace" });
        break;
      case "sources":
        provider.setSources(command.recording, command.items);
        emit({ event: "ack", cmd: "sources" });
        break;
      case "drop":
        emit({ event: "dropped", at_ms: provider.drop(command.source) });
        break;
      case "streams":
        emit({ event: "streams", count: provider.streamsOpen(command.source) });
        break;
      case "stats":
        emit({ event: "stats", counters: provider.counters });
        break;
      case "close":
        await provider.close();
        process.exit(0);
    }
  });
  lines.on("close", async () => {
    await provider.close();
    process.exit(0);
  });
}
