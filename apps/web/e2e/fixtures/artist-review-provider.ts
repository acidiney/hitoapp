import {
  createServer,
  type IncomingMessage,
  type ServerResponse,
} from "node:http";
import type { AddressInfo } from "node:net";
import { isDeepStrictEqual } from "node:util";
import type { components } from "../../src/generated";
import { artistDiscographyData } from "./artist-discography-data";

type Schema = components["schemas"];

export async function artistReviewProvider(
  query: string,
  options: { discography?: boolean } = {},
) {
  if (process.env.HITO_ARTIST_REVIEW_E2E !== "1")
    throw new Error("The disposable artist provider requires explicit opt-in");
  if (options.discography && process.env.HITO_ARTIST_DISCOGRAPHY_E2E !== "1")
    throw new Error(
      "The disposable discography provider requires explicit opt-in",
    );
  const token = "owned-browser-private-artist-provider-token";
  const manifest = {
    id: "org.hito.browser.artist-review",
    name: "Owned browser artist catalog",
    protocol_version: 1,
    capabilities: ["catalog"],
  } satisfies Schema["Manifest"];
  const candidates = [
    {
      artist: {
        id: "opaque:artist/first?credit=shared",
        name: "Owned namesake",
      },
      kind: "group",
      disambiguation: "Electronic ensemble from Lisbon",
    },
    {
      artist: {
        id: "opaque:artist/second?credit=shared~á",
        name: "Owned namesake",
      },
      kind: "person",
      disambiguation: "Acoustic singer from Luanda",
    },
  ] satisfies Schema["ArtistSearchCandidate"][];
  const profile = {
    artist: candidates[1].artist,
    sort_name: "Namesake, Owned",
    kind: "person",
    disambiguation: "Acoustic singer from Luanda, established in 2012",
    revision: "owned-browser-artist-profile:1",
  } satisfies Schema["CatalogArtistProfile"];
  const search = {
    query,
    cursor: null,
    snapshot: null,
    limit: 20,
  } satisfies Schema["ArtistSearchRequest"];
  const discography = options.discography
    ? artistDiscographyData(profile.artist)
    : null;
  const calls: string[] = [];
  const counters = { unexpected: 0, unsafeHeaders: 0, requests: 0 };
  const active = new Set<Promise<void>>();
  async function handle(request: IncomingMessage, response: ServerResponse) {
    const reject = () => {
      counters.unexpected++;
      response.writeHead(400).end();
    };
    if (++counters.requests > (discography ? 20 : 16)) return reject();
    const path = request.url ?? "";
    const isManifest = path === "/catalog/v1/manifest";
    const authorization = request.headers.authorization;
    if (
      request.headers.cookie ||
      request.headers["x-hito-plugin-protocol"] !== "1" ||
      (authorization !== `Bearer ${token}` && !(isManifest && !authorization))
    ) {
      counters.unsafeHeaders++;
      return reject();
    }
    const chunks: Buffer[] = [];
    let length = 0;
    for await (const chunk of request) {
      length += Buffer.byteLength(chunk);
      if (length > 2048) return reject();
      chunks.push(Buffer.from(chunk));
    }
    const bytes = Buffer.concat(chunks);
    const body: unknown = bytes.length ? JSON.parse(bytes.toString()) : null;
    const reply = (kind: string, value: unknown) => {
      calls.push(kind);
      response
        .writeHead(200, { "content-type": "application/json" })
        .end(JSON.stringify(value));
    };
    if (request.method === "GET" && !bytes.length) {
      if (isManifest)
        return reply(
          authorization ? "manifest_private" : "manifest_public",
          manifest,
        );
      if (path === "/catalog/v1/catalog/discovery/capabilities")
        return reply("discovery", {
          discovery_version: 1,
          operations: ["artist_search"],
        });
      if (path === "/catalog/v1/catalog/capabilities")
        return reply("profile_descriptor", {
          catalog_version: 1,
          operations: discography
            ? ["artist_profile", "artist_discography"]
            : ["artist_profile"],
        });
    }
    if (request.method === "POST") {
      if (discography && path === "/catalog/v1/catalog/artists/discography") {
        for (const [kind, cursor, page, maximum] of [
          ["discography_first", null, discography.first, 2],
          ["discography_next", discography.cursor, discography.last, 1],
        ] as const) {
          if (
            isDeepStrictEqual(body, {
              artist_id: profile.artist.id,
              cursor,
              snapshot: null,
              limit: 20,
            }) &&
            calls.filter((call) => call === kind).length < maximum
          )
            return reply(kind, page);
        }
        return reject();
      }
      if (
        path === "/catalog/v1/catalog/artists/search" &&
        isDeepStrictEqual(body, search)
      ) {
        const page = {
          query,
          snapshot: "owned-browser-artist-search:1",
          items: candidates,
          total_count: 2,
          next_cursor: null,
        } satisfies Schema["ArtistSearchPage"];
        return reply("search", page);
      }
      if (
        path === "/catalog/v1/catalog/artists/profile" &&
        isDeepStrictEqual(body, { artist_id: profile.artist.id })
      )
        return reply("profile", profile);
    }
    reject();
  }
  const server = createServer((request, response) => {
    const task = handle(request, response)
      .catch(() => {
        counters.unexpected++;
        response.destroy();
      })
      .finally(() => active.delete(task));
    active.add(task);
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
  const origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  return {
    token,
    manifest,
    search,
    candidates,
    profile,
    discography,
    origin,
    address: `${origin}/catalog/`,
    calls,
    counters,
    async close() {
      try {
        await new Promise<void>((resolve, reject) => {
          const timer = setTimeout(
            () => reject(new Error("Owned artist provider close timed out")),
            3000,
          );
          server.close((error) => {
            clearTimeout(timer);
            if (error) reject(new Error("Owned artist provider close failed"));
            else resolve();
          });
          server.closeAllConnections();
        });
      } finally {
        await Promise.allSettled([...active]);
      }
    },
  };
}
