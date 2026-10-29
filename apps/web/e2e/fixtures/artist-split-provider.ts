import { createServer } from "node:http";
import type { AddressInfo } from "node:net";

/** An owned catalog service with three recordings that share one credit and two artist identities. */
export const artistSplit = {
  credit: "Split performer",
  artists: ["e2e:artist/split-first", "e2e:artist/split-second"],
  recordings: [
    "e2e:recording/split-a",
    "e2e:recording/split-b",
    "e2e:recording/split-c",
  ],
};

export async function artistSplitProvider(label: string) {
  const token = "browser-private-artist-split-token";
  const titles = Object.fromEntries(
    artistSplit.recordings.map((id, index) => [
      id,
      `Split track ${index + 1} ${label}`,
    ]),
  );
  const state = { unexpected: 0 };
  const server = createServer(async (request, response) => {
    if (request.headers.authorization !== `Bearer ${token}`) {
      response.writeHead(401).end();
      return;
    }
    const route = new URL(
      request.url!,
      "http://fixture.invalid",
    ).pathname.replace(/^\/split\//, "");
    let body = "";
    for await (const chunk of request) body += chunk;
    const value = body ? JSON.parse(body) : null;
    const reply = (payload: unknown) =>
      response
        .writeHead(200, { "content-type": "application/json" })
        .end(JSON.stringify(payload));
    switch (route) {
      case "v1/manifest":
        return reply({
          id: "org.hito.artist-split-e2e",
          name: `Artist split fixture ${label}`,
          protocol_version: 1,
          capabilities: ["catalog", "search"],
        });
      case "v1/catalog/capabilities":
        return reply({ catalog_version: 1, operations: ["artist_profile"] });
      case "v1/catalog/discovery/capabilities":
        return reply({ discovery_version: 1, operations: ["artist_search"] });
      case "v1/catalog/artists/profile": {
        if (!artistSplit.artists.includes(value?.artist_id)) {
          state.unexpected++;
          return response.writeHead(400).end();
        }
        return reply({
          artist: { id: value.artist_id, name: artistSplit.credit },
          sort_name: null,
          kind: "person",
          disambiguation: value.artist_id.endsWith("first")
            ? "First"
            : "Second",
          revision: "split-profile:1",
        });
      }
      case "v1/recordings/detail": {
        const id = value?.recording_id as string;
        if (!(id in titles)) {
          state.unexpected++;
          return response.writeHead(400).end();
        }
        return reply({
          id,
          title: titles[id],
          artist: artistSplit.credit,
          duration_ms: 1000,
          release: { id: "e2e:release/split", title: `Split album ${label}` },
          disc_number: 1,
          track_number: artistSplit.recordings.indexOf(id) + 1,
          revision: "split-r1",
        });
      }
      default:
        state.unexpected++;
        response.writeHead(404).end();
    }
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  return {
    titles,
    token,
    state,
    address: `http://127.0.0.1:${(server.address() as AddressInfo).port}/split/`,
    close: async () => {
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      );
    },
  };
}
