import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { serveAudio } from "./album-edition-provider";

export const catalogAlbum = {
  artistId: "e2e:artist/catalog-album",
  artistName: "Catalog album performer",
  groupId: "e2e:group/catalog-album",
  editionId: "e2e:edition/catalog-album",
  groupTitle: "Catalog play album",
  anchor: { id: "e2e:recording/anchor", title: "Anchor track" },
  tracks: [
    "Opening movement",
    "Quiet interlude",
    "Hidden gem",
    "Closing movement",
  ],
};

export const catalogOtherReleases = [
  { title: "Second light", kind: "album", date: "2021-03-05" },
  { title: "Night shift sessions", kind: "album", date: "2016-10-28" },
  { title: "Small hours", kind: "ep", date: "2018-02-09" },
  { title: "Harbour", kind: "ep", date: "2014-06-13" },
  { title: "Paper boats", kind: "single", date: "2022-01-21" },
  { title: "Long way round (live)", kind: "single", date: "2020-09-11" },
] as const;

export type CatalogAlbumState = {
  available: Set<string>;
  failing: Set<string>;
  outage: boolean;
  searches: string[];
  unexpected: number;
};

export async function catalogAlbumProvider() {
  const audio = await readFile(
    "/workspace/fixtures/e2e-generated/Music/Fixture 440 Hz.wav",
  );
  const token = "browser-private-catalog-album-token";
  const state: CatalogAlbumState = {
    available: new Set(catalogAlbum.tracks),
    failing: new Set(),
    outage: false,
    searches: [],
    unexpected: 0,
  };
  const audioRequests: string[] = [];
  const idOf = (title: string) => `e2e:audio/${title}`;
  const titleOf = (id: string) =>
    catalogAlbum.tracks.find((title) => idOf(title) === id) ??
    (id === catalogAlbum.anchor.id ? catalogAlbum.anchor.title : null);
  const detail = (title: string, id = idOf(title)) => ({
    id,
    title,
    artist: catalogAlbum.artistName,
    duration_ms: 1000,
    release: null,
    disc_number: null,
    track_number: null,
    revision: "catalog-album-r1",
  });
  const occurrence = (title: string, index: number) => ({
    id: `e2e:occ/${index + 1}`,
    position: index + 1,
    number: String(index + 1),
    kind: "audio",
    title,
    artist_credit: [
      {
        artist: null,
        credited_name: catalogAlbum.artistName,
        join_phrase: "",
      },
    ],
    duration_ms: 1000,
    recording_id: null,
  });
  const server = createServer(async (request, response) => {
    const url = new URL(request.url!, "http://fixture.invalid");
    if (request.headers.authorization !== `Bearer ${token}`) {
      response.writeHead(401).end();
      return;
    }
    const route = url.pathname.replace(/^\/catalog-album\//, "");
    if (route === "v1/audio") {
      audioRequests.push(url.searchParams.get("source_id") ?? "");
      serveAudio(request, response, audio);
      return;
    }
    let body = "";
    for await (const chunk of request) body += chunk;
    const value = body ? JSON.parse(body) : null;
    const reply = (payload: unknown) =>
      response
        .writeHead(200, { "content-type": "application/json" })
        .end(JSON.stringify(payload));
    if ((state.outage && route !== "v1/manifest") || state.failing.has(route))
      return response.writeHead(503).end();
    switch (route) {
      case "v1/manifest":
        return reply({
          id: "org.hito.catalog-album-e2e",
          name: "Catalog album fixture",
          protocol_version: 1,
          capabilities: ["catalog", "search", "sources", "metadata"],
        });
      case "v1/catalog/capabilities":
        return reply({
          catalog_version: 1,
          operations: ["artist_profile", "artist_discography"],
        });
      case "v1/catalog/editions/capabilities":
        return reply({
          edition_version: 1,
          operations: ["release_group_editions", "edition_track_list"],
        });
      case "v1/catalog/artists/profile":
        return reply({
          artist: {
            id: catalogAlbum.artistId,
            name: catalogAlbum.artistName,
          },
          sort_name: null,
          kind: null,
          disambiguation: null,
          revision: null,
        });
      case "v1/catalog/artists/discography":
        return reply({
          artist_id: catalogAlbum.artistId,
          snapshot: null,
          total_count: 1 + catalogOtherReleases.length,
          next_cursor: null,
          items: [
            ...catalogOtherReleases.map((release, index) => ({
              id: `e2e:group/other-${index}`,
              title: release.title,
              artist_credit: [
                {
                  artist: null,
                  credited_name: catalogAlbum.artistName,
                  join_phrase: "",
                },
              ],
              kind: release.kind,
              secondary_types: [],
              first_release_date: release.date,
              revision: null,
            })),
            {
              id: catalogAlbum.groupId,
              title: catalogAlbum.groupTitle,
              artist_credit: [
                {
                  artist: null,
                  credited_name: catalogAlbum.artistName,
                  join_phrase: "",
                },
              ],
              kind: "album",
              secondary_types: [],
              first_release_date: "2019-05-17",
              revision: null,
            },
          ],
        });
      case "v1/catalog/release-groups/editions":
        return reply({
          release_group_id: catalogAlbum.groupId,
          snapshot: null,
          total_count: 1,
          next_cursor: null,
          items: [
            {
              id: catalogAlbum.editionId,
              title: catalogAlbum.groupTitle,
              artist_credit: null,
              disambiguation: null,
              release_date: "2019-05-17",
              country: "XW",
              revision: null,
            },
          ],
        });
      case "v1/catalog/editions/track-list":
        return reply({
          id: catalogAlbum.editionId,
          release_group_id: catalogAlbum.groupId,
          title: catalogAlbum.groupTitle,
          artist_credit: null,
          revision: null,
          media: [
            {
              position: 1,
              title: null,
              format: "CD",
              tracks: catalogAlbum.tracks.map(occurrence),
            },
          ],
        });
      case "v1/search": {
        state.searches.push(String(value.query));
        const wanted = [...state.available]
          .filter((title) => String(value.query).includes(title))
          .sort((a, b) => b.length - a.length)[0];
        return reply({
          items: wanted ? [detail(wanted)] : [],
          next_cursor: null,
        });
      }
      case "v1/recordings/detail": {
        const title = titleOf(value.recording_id);
        if (!title) break;
        return reply(detail(title, value.recording_id));
      }
      case "v1/recordings/metadata": {
        const title = titleOf(value.recording_id);
        if (!title) break;
        return reply({
          recording: detail(title, value.recording_id),
          musical_title: title,
          album_title: catalogAlbum.groupTitle,
          source_ids: [`${value.recording_id}/source`],
        });
      }
      case "v1/sources":
        return reply({
          items: [
            {
              id: `${value.recording_id}/source`,
              label: "Original",
              ready: true,
            },
          ],
        });
    }
    state.unexpected++;
    response.writeHead(404).end();
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  return {
    address: `http://127.0.0.1:${(server.address() as AddressInfo).port}/catalog-album/`,
    token,
    state,
    audioRequests,
    close: async () => {
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      );
    },
  };
}
