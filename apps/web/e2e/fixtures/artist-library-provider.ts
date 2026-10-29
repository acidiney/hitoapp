import { createServer } from "node:http";
import type { AddressInfo } from "node:net";

/** An owned catalog service with one artist, one owned recording and a discography of every primary type. */
export const artistLibrary = {
  credit: "Library performer",
  artist: "e2e:artist/library",
  recording: "e2e:recording/library-a",
  linkedGroup: "e2e:group/linked-album",
  groups: [
    { id: "e2e:group/linked-album", title: "Linked album", kind: "album" },
    { id: "e2e:group/other-album", title: "Linked album", kind: "album" },
    { id: "e2e:group/ep", title: "An EP", kind: "ep" },
    { id: "e2e:group/single", title: "A single", kind: "single" },
    { id: "e2e:group/broadcast", title: "A broadcast", kind: "broadcast" },
  ],
};

export async function artistLibraryProvider(label: string) {
  const token = "browser-private-artist-library-token";
  const title = `Library track ${label}`;
  const state = { unexpected: 0, outage: false, discographyReads: 0 };
  const server = createServer(async (request, response) => {
    if (request.headers.authorization !== `Bearer ${token}`) {
      response.writeHead(401).end();
      return;
    }
    const route = new URL(request.url!, "http://fixture.invalid").pathname.replace(
      /^\/library\//,
      "",
    );
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
          id: "org.hito.artist-library-e2e",
          name: `Artist library fixture ${label}`,
          protocol_version: 1,
          capabilities: ["catalog", "search"],
        });
      case "v1/catalog/capabilities":
        return reply({
          catalog_version: 1,
          operations: ["artist_profile", "artist_discography"],
        });
      case "v1/catalog/discovery/capabilities":
        return reply({ discovery_version: 1, operations: ["artist_search"] });
      case "v1/catalog/artists/profile":
        if (value?.artist_id !== artistLibrary.artist) {
          state.unexpected++;
          return response.writeHead(400).end();
        }
        return reply({
          artist: { id: value.artist_id, name: artistLibrary.credit },
          sort_name: null,
          kind: "person",
          disambiguation: null,
          revision: "library-profile:1",
        });
      case "v1/catalog/artists/discography": {
        if (value?.artist_id !== artistLibrary.artist) {
          state.unexpected++;
          return response.writeHead(400).end();
        }
        state.discographyReads++;
        if (state.outage) return response.writeHead(503).end();
        return reply({
          artist_id: artistLibrary.artist,
          snapshot: null,
          total_count: null,
          next_cursor: null,
          items: artistLibrary.groups.map((group) => ({
            ...group,
            artist_credit: null,
            secondary_types: [],
            first_release_date: null,
            revision: null,
          })),
        });
      }
      case "v1/recordings/detail": {
        if (value?.recording_id !== artistLibrary.recording) {
          state.unexpected++;
          return response.writeHead(400).end();
        }
        return reply({
          id: artistLibrary.recording,
          title,
          artist: artistLibrary.credit,
          duration_ms: 1000,
          release: { id: "e2e:release/library", title: `Library album ${label}` },
          disc_number: 1,
          track_number: 1,
          revision: "library-r1",
        });
      }
      default:
        state.unexpected++;
        response.writeHead(404).end();
    }
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  return {
    title,
    token,
    state,
    address: `http://127.0.0.1:${(server.address() as AddressInfo).port}/library/`,
    close: async () => {
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      );
    },
  };
}
