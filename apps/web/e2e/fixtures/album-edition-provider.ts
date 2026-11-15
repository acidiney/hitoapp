import { readFile } from "node:fs/promises";
import {
  createServer,
  type IncomingMessage,
  type ServerResponse,
} from "node:http";
import type { AddressInfo } from "node:net";

export const albumEdition = {
  artistId: "e2e:artist/album-owner",
  artistName: "Album edition performer",
  groupId: "e2e:group/album",
  deluxeId: "e2e:edition/deluxe",
  standardId: "e2e:edition/standard",
  first: "e2e:recording/one",
  second: "e2e:recording/two",
  bonus: "e2e:recording/bonus",
  releaseTitle: "Album edition fixture",
};

function occurrence(
  id: string,
  position: number,
  title: string,
  recording: string | null,
) {
  return {
    id,
    position,
    number: String(position),
    kind: "audio",
    title,
    artist_credit: [
      {
        artist: null,
        credited_name: albumEdition.artistName,
        join_phrase: "",
      },
    ],
    duration_ms: 1000,
    recording_id: recording,
  };
}

export const unidentified = "Catalog unidentified";

const deluxe = {
  id: albumEdition.deluxeId,
  release_group_id: albumEdition.groupId,
  title: albumEdition.releaseTitle,
  artist_credit: null,
  revision: null,
  media: [
    {
      position: 1,
      title: "Main",
      format: "CD",
      tracks: [
        occurrence("e2e:occ/1/1", 1, "Catalog one", albumEdition.first),
        occurrence("e2e:occ/1/2", 2, "Catalog two", albumEdition.second),
      ],
    },
    {
      position: 2,
      title: "Bonus",
      format: null,
      tracks: [
        occurrence("e2e:occ/2/1", 1, "Catalog bonus", albumEdition.bonus),
        occurrence("e2e:occ/2/2", 2, "Catalog one again", albumEdition.first),
      ],
    },
  ],
};

const standard = {
  ...deluxe,
  id: albumEdition.standardId,
  media: [{ ...deluxe.media[0], tracks: [deluxe.media[0].tracks[0]] }],
};

export function serveAudio(
  request: IncomingMessage,
  response: ServerResponse,
  audio: Buffer,
) {
  let first = 0;
  let last = audio.length - 1;
  const range = request.method !== "HEAD" ? request.headers.range : undefined;
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
}

export async function albumEditionProvider(
  label: string,
  unidentifiedTrack = false,
) {
  const titles: Record<string, string> = {
    [albumEdition.first]: `Album edition one ${label}`,
    [albumEdition.second]: `Album edition two ${label}`,
    [albumEdition.bonus]: `Album edition bonus ${label}`,
  };
  const audio = await readFile(
    "/workspace/fixtures/e2e-generated/Music/Fixture 440 Hz.wav",
  );
  const token = "browser-private-album-edition-token";
  const calls: string[] = [];
  const details: string[] = [];
  const audioRequests: string[] = [];
  const state = {
    outage: false,
    unexpected: 0,
    durations: null as (number | null)[] | null,
  };
  const timed = (edition: typeof deluxe) => {
    const durations = state.durations;
    if (!durations) return edition;
    let index = 0;
    return {
      ...edition,
      media: edition.media.map((medium) => ({
        ...medium,
        tracks: medium.tracks.map((track) => ({
          ...track,
          duration_ms: durations[index++] ?? null,
        })),
      })),
    };
  };
  const sourceFor = (id: string) => `e2e:source/${id}`;
  const server = createServer(async (request, response) => {
    const url = new URL(request.url!, "http://fixture.invalid");
    if (request.headers.authorization !== `Bearer ${token}`) {
      response.writeHead(401).end();
      return;
    }
    const route = url.pathname.replace(/^\/albums\//, "");
    if (route === "v1/audio") {
      audioRequests.push(url.searchParams.get("source_id") ?? "");
      serveAudio(request, response, audio);
      return;
    }
    let body = "";
    for await (const chunk of request) body += chunk;
    const value = body ? JSON.parse(body) : null;
    calls.push(route);
    const reply = (payload: unknown) =>
      response
        .writeHead(200, { "content-type": "application/json" })
        .end(JSON.stringify(payload));
    switch (route) {
      case "v1/manifest":
        return reply({
          id: "org.hito.album-edition-e2e",
          name: "Album edition fixture",
          protocol_version: 1,
          capabilities: ["catalog", "search", "sources"],
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
          artist: { id: albumEdition.artistId, name: albumEdition.artistName },
          sort_name: null,
          kind: null,
          disambiguation: null,
          revision: null,
        });
      case "v1/catalog/artists/discography":
        return reply({
          artist_id: albumEdition.artistId,
          snapshot: null,
          total_count: 1,
          next_cursor: null,
          items: [
            {
              id: albumEdition.groupId,
              title: albumEdition.releaseTitle,
              artist_credit: null,
              kind: "album",
              secondary_types: [],
              first_release_date: null,
              revision: null,
            },
          ],
        });
      case "v1/catalog/release-groups/editions":
        return reply({
          release_group_id: albumEdition.groupId,
          snapshot: null,
          total_count: 2,
          next_cursor: null,
          items: [albumEdition.deluxeId, albumEdition.standardId].map((id) => ({
            id,
            title: albumEdition.releaseTitle,
            artist_credit: null,
            disambiguation:
              id === albumEdition.deluxeId ? "Deluxe" : "Standard",
            release_date: null,
            country: null,
            revision: null,
          })),
        });
      case "v1/catalog/editions/track-list": {
        if (state.outage) return response.writeHead(503).end();
        const edition =
          value.edition_id === albumEdition.deluxeId
            ? deluxe
            : value.edition_id === albumEdition.standardId
              ? standard
              : null;
        if (!edition || value.release_group_id !== albumEdition.groupId) {
          state.unexpected++;
          return response.writeHead(400).end();
        }
        if (unidentifiedTrack && edition === deluxe)
          return reply({
            ...deluxe,
            media: [
              deluxe.media[0],
              {
                ...deluxe.media[1],
                tracks: [
                  ...deluxe.media[1].tracks,
                  occurrence("e2e:occ/2/3", 3, unidentified, null),
                ],
              },
            ],
          });
        return reply(timed(edition));
      }
      case "v1/recordings/detail": {
        const id = value.recording_id as string;
        if (!(id in titles)) {
          state.unexpected++;
          return response.writeHead(400).end();
        }
        details.push(id);
        return reply({
          id,
          title: titles[id],
          artist: albumEdition.artistName,
          duration_ms: 1000,
          release: {
            id: albumEdition.deluxeId,
            title: albumEdition.releaseTitle,
            artist: albumEdition.artistName,
          },
          disc_number: id === albumEdition.bonus ? 2 : 1,
          track_number: id === albumEdition.second ? 2 : 1,
          revision: "album-edition-r1",
        });
      }
      case "v1/search":
        return reply({ items: [], next_cursor: null });
      case "v1/sources":
        return reply({
          items: [
            {
              id: sourceFor(value.recording_id),
              label: "Original",
              ready: true,
            },
          ],
        });
      default:
        state.unexpected++;
        response.writeHead(404).end();
    }
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  return {
    titles,
    address: `http://127.0.0.1:${(server.address() as AddressInfo).port}/albums/`,
    token,
    calls,
    details,
    audioRequests,
    state,
    close: async () => {
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      );
    },
  };
}
