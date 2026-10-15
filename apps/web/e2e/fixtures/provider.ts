import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";

export async function providerFixture() {
  const audio = await readFile(
    "/workspace/fixtures/e2e-generated/Music/Fixture 440 Hz.wav",
  );
  const token = "browser-private-relay-token";
  const recording = {
    id: "provider/private/recording",
    title: "Relay owned fixture",
    artist: "Hito test fixtures",
    duration_ms: 90000,
  };
  const source = "provider:/audio?private=value&other=bad";
  const requests: {
    authorization?: string;
    cookie?: string;
    source: string | null;
  }[] = [];
  const server = createServer(async (request, response) => {
    const url = new URL(request.url!, "http://fixture.invalid");
    if (request.headers.authorization !== `Bearer ${token}`) {
      response.writeHead(401).end();
      return;
    }
    if (url.pathname === "/fixture/v1/audio") {
      requests.push({
        authorization: request.headers.authorization,
        cookie: request.headers.cookie,
        source: url.searchParams.get("source_id"),
      });
      if (url.searchParams.get("source_id") !== source) {
        response.writeHead(404).end();
        return;
      }
      let first = 0;
      let last = audio.length - 1;
      const range =
        request.method !== "HEAD" ? request.headers.range : undefined;
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
        request.method === "HEAD" ? undefined : audio.subarray(first, last + 1),
      );
      return;
    }
    let body = "";
    for await (const chunk of request) {
      body += chunk;
    }
    let reply: unknown;
    switch (url.pathname) {
      case "/fixture/v1/manifest":
        reply = {
          id: "org.hito.relay-fixture",
          name: "Relay fixture",
          protocol_version: 1,
          capabilities: ["search", "sources"],
        };
        break;
      case "/fixture/v1/search":
        reply = { items: [recording] };
        break;
      case "/fixture/v1/recordings/detail":
        reply =
          JSON.parse(body).recording_id === recording.id ? recording : null;
        break;
      case "/fixture/v1/sources":
        reply = { items: [{ id: source, label: "Original", ready: true }] };
        break;
      default:
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
    token,
    recording,
    source,
    requests,
    close: async () => {
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      );
    },
  };
}
