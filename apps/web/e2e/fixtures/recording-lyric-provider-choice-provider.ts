import { randomUUID } from "node:crypto";
import { createServer, type IncomingMessage } from "node:http";
import type { AddressInfo } from "node:net";
import { isDeepStrictEqual } from "node:util";
import type { Schema } from "../../src/shared/schema";

async function readBody(request: IncomingMessage) {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of request) {
    if ((size += Buffer.byteLength(chunk)) > 4096)
      throw new Error("Owned choice request exceeded its bound");
    chunks.push(Buffer.from(chunk));
  }
  return size
    ? (JSON.parse(Buffer.concat(chunks).toString()) as unknown)
    : null;
}

export async function lyricChoiceProviders(query: string) {
  if (process.env.HITO_LYRIC_PROVIDER_CHOICE_E2E !== "1")
    throw new Error("The disposable provider-choice guard is required");
  const nonce = randomUUID();
  const title = `Owned lyric choice ${nonce}`;
  const artist = `Owned choice performer ${nonce}`;
  const genericQuery = `${artist} ${title}`;
  async function start(role: "primary" | "attached") {
    const token = `owned-choice-${role}-${randomUUID()}`;
    const manifest = {
      id: `org.hito.browser.lyric-choice.${role}`,
      name: `Owned ${role} lyric provider ${nonce}`,
      protocol_version: 1,
      capabilities: ["search", "sources", "lyrics"],
      content_policy: { source_persistence: "playback_only" },
    } satisfies Schema<"Manifest">;
    const recording = {
      id: `owned/${role}?recording="${nonce}"`,
      title,
      artist,
      duration_ms: 12_000,
      release: null,
      disc_number: null,
      track_number: null,
      revision: `owned-${role}-revision:"${nonce}"`,
    } satisfies Schema<"ProviderRecording">;
    const reference = {
      id: recording.id,
      revision: recording.revision,
    } satisfies Schema<"ProviderRecordingRef">;
    const lyric = {
      id: `owned-${role}-lyric-${nonce}`,
      rendition_id: `owned-${role}-rendition-${nonce}`,
      revision: `owned-${role}-lyric-revision:"${nonce}"`,
      rendition_label: `Owned ${role} static rendition ${nonce}`,
      language: "en",
      duration_ms: 3000,
      attribution: {
        creators: [`Owned ${role} lyric creator ${nonce}`],
        provider_label: `Owned ${role} lyric credits ${nonce}`,
        content_url: `https://credits.example.invalid/${role}/${nonce}`,
      },
      lines: [
        {
          start_ms: 0,
          end_ms: 1200,
          text: `Owned ${role} opening line ${nonce}`,
        },
        {
          start_ms: 1500,
          end_ms: 2400,
          text: `<img src="https://lyrics.example.invalid/${role}/${nonce}" onerror="window.ownedChoicePayload=true">`,
        },
      ],
    } satisfies Schema<"TimedLyrics">;
    const calls = {
      total: 0,
      publicManifest: 0,
      privateManifest: 0,
      search: 0,
      detail: 0,
      sources: 0,
      descriptor: 0,
      lyrics: 0,
      audio: 0,
      unexpected: 0,
      unsafeHeaders: 0,
    };
    const lyricRequests: Pick<Schema<"LyricsResponse">, "recording">[] = [];
    const tasks = new Set<Promise<void>>();
    let armed = false;
    let closing = false;
    const server = createServer((request, response) => {
      const work = (async () => {
        const reject = (status = 400) => {
          calls.unexpected++;
          response.writeHead(status).end();
        };
        response.on("error", () => {
          if (!closing) calls.unexpected++;
        });
        if (++calls.total > 40) return reject(429);
        const path = request.url ?? "";
        const isManifest = path === "/choice/v1/manifest";
        const authorization = request.headers.authorization;
        if (
          request.headers.cookie !== undefined ||
          request.headers.range !== undefined ||
          request.headers["x-hito-plugin-protocol"] !== "1" ||
          (authorization !== `Bearer ${token}` &&
            !(isManifest && authorization === undefined))
        ) {
          calls.unsafeHeaders++;
          return reject(401);
        }
        const sent = await readBody(request);
        let reply: unknown;
        if (path.includes("/audio")) {
          calls.audio++;
          return reject();
        }
        if (
          armed &&
          !["/choice/v1/lyrics/capabilities", "/choice/v1/lyrics"].includes(
            path,
          )
        )
          return reject();
        if (request.method === "GET" && sent === null && isManifest) {
          calls[authorization ? "privateManifest" : "publicManifest"]++;
          reply = manifest;
        } else if (request.method === "POST" && path === "/choice/v1/search") {
          const discover = isDeepStrictEqual(sent, {
            query,
            cursor: null,
            limit: 20,
          });
          const generic = isDeepStrictEqual(sent, {
            query: genericQuery,
            cursor: null,
            limit: 20,
          });
          if (!(role === "primary" ? discover || generic : generic))
            return reject();
          calls.search++;
          reply = {
            items: role === "attached" || discover ? [recording] : [],
            next_cursor: null,
          } satisfies Schema<"SearchPage">;
        } else if (
          request.method === "POST" &&
          path === "/choice/v1/recordings/detail" &&
          isDeepStrictEqual(sent, { recording_id: recording.id })
        ) {
          calls.detail++;
          reply = recording;
        } else if (
          request.method === "POST" &&
          path === "/choice/v1/sources" &&
          isDeepStrictEqual(sent, { recording_id: recording.id })
        ) {
          calls.sources++;
          reply = {
            items:
              role === "attached"
                ? [
                    {
                      id: `owned-source-${nonce}`,
                      label: "Owned attached source",
                      ready: true,
                    },
                  ]
                : [],
          };
        } else if (
          request.method === "GET" &&
          path === "/choice/v1/lyrics/capabilities" &&
          sent === null
        ) {
          calls.descriptor++;
          reply = { lyrics_version: 1 };
        } else if (
          request.method === "POST" &&
          path === "/choice/v1/lyrics" &&
          isDeepStrictEqual(sent, { recording: reference })
        ) {
          if (lyricRequests.length >= 2) return reject(429);
          calls.lyrics++;
          lyricRequests.push(
            sent as Pick<Schema<"LyricsResponse">, "recording">,
          );
          reply = {
            recording: reference,
            lyrics: lyric,
          } satisfies Schema<"LyricsResponse">;
        } else return reject(404);
        const bytes = JSON.stringify(reply);
        if (Buffer.byteLength(bytes) > 16_384) return reject(500);
        response
          .writeHead(200, { "content-type": "application/json" })
          .end(bytes);
      })()
        .catch(() => {
          if (!closing) calls.unexpected++;
          response.destroy();
        })
        .finally(() => tasks.delete(work));
      tasks.add(work);
    });
    server.requestTimeout = 4000;
    server.headersTimeout = 4000;
    server.maxHeadersCount = 32;
    server.maxConnections = 8;
    server.setTimeout(8000, (socket) => socket.destroy());
    server.on("clientError", (_error, socket) => {
      if (!closing) calls.unexpected++;
      socket.destroy();
    });
    await new Promise<void>((resolve, reject) => {
      server.once("error", reject);
      server.listen(0, "127.0.0.1", () => {
        server.off("error", reject);
        resolve();
      });
    });
    server.on("error", () => {
      if (!closing) calls.unexpected++;
    });
    const origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
    return {
      token,
      manifest,
      recording,
      reference,
      lyric,
      calls,
      lyricRequests,
      address: `${origin}/choice/`,
      privateValues: [
        token,
        origin,
        manifest.id,
        manifest.name,
        recording.id,
        recording.revision,
        lyric.id,
        lyric.rendition_id,
        lyric.revision,
        lyric.rendition_label,
        ...lyric.lines.map((line) => line.text),
        ...lyric.attribution.creators,
        lyric.attribution.provider_label,
        lyric.attribution.content_url,
      ],
      arm() {
        armed = true;
      },
      async close() {
        closing = true;
        const stopped = new Promise<void>((resolve, reject) =>
          server.close((error) =>
            error
              ? reject(new Error("Owned choice fixture close failed"))
              : resolve(),
          ),
        );
        server.closeAllConnections();
        await Promise.all([stopped, ...tasks]);
      },
    };
  }
  const primary = await start("primary");
  try {
    const attached = await start("attached");
    return {
      primary,
      attached,
      privateValues: [
        query,
        title,
        artist,
        genericQuery,
        "ownedChoicePayload",
        ...primary.privateValues,
        ...attached.privateValues,
      ],
    };
  } catch (error) {
    await primary.close();
    throw error;
  }
}
