import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";

export async function providerFixture(prefix = "Relay", expanded = false) {
  const audio = await readFile(
    "/workspace/fixtures/e2e-generated/Music/Fixture 440 Hz.wav",
  );
  const token = "browser-private-relay-token";
  const recording = {
    id: "provider/private/recording",
    title: `${prefix} owned fixture`,
    artist: "Hito test fixtures",
    duration_ms: 90000,
  };
  const source = "provider:/audio?private=value&other=bad";
  const second = {
    ...recording,
    id: "provider/private/second",
    title: `${prefix} second fixture`,
  };
  const secondSource = `${source}/second`;
  const extras = expanded
    ? Array.from({ length: 5 }, (_, index) => ({
        ...recording,
        id: `provider/private/catalog-${index}`,
        title: `${prefix} catalog fixture ${index + 1}`,
      }))
    : [];
  const records = new Map(
    [recording, second, ...extras].map((item) => [item.id, item]),
  );
  const sourceFor = (id: string) =>
    id === recording.id
      ? source
      : id === second.id
        ? secondSource
        : `${source}/${id}`;
  const sources = new Set([...records.keys()].map(sourceFor));
  const gates = new Map<
    string,
    { entered: boolean; wait: Promise<void>; release: () => void }
  >();
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
      if (!sources.has(url.searchParams.get("source_id") ?? "")) {
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
        reply =
          JSON.parse(body).cursor === "browser-private-next"
            ? { items: [second] }
            : {
                items: [recording, ...extras],
                next_cursor: "browser-private-next",
              };
        break;
      case "/fixture/v1/recordings/detail": {
        const id = JSON.parse(body).recording_id;
        const gate = gates.get(id);
        if (gate) {
          gate.entered = true;
          await gate.wait;
          gates.delete(id);
        }
        reply = records.get(id) ?? null;
        break;
      }
      case "/fixture/v1/sources":
        reply = {
          items: [
            {
              id: sourceFor(JSON.parse(body).recording_id),
              label: "Original",
              ready: true,
            },
          ],
        };
        break;
      default:
        response.writeHead(404).end();
        return;
    }
    if (response.destroyed) return;
    response
      .writeHead(200, { "content-type": "application/json" })
      .end(JSON.stringify(reply));
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  return {
    address: `http://127.0.0.1:${(server.address() as AddressInfo).port}/fixture/`,
    token,
    recording,
    second,
    source,
    secondSource,
    requests,
    holdDetails: (id: string) => {
      let release!: () => void;
      const wait = new Promise<void>((done) => {
        release = done;
      });
      const gate = { entered: false, wait, release };
      gates.set(id, gate);
      return {
        get entered() {
          return gate.entered;
        },
        release,
      };
    },
    close: async () => {
      gates.forEach((gate) => gate.release());
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      );
    },
  };
}
