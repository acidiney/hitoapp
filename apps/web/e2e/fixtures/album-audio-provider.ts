import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import {
  albumEdition,
  serveAudio,
  unidentified,
} from "./album-edition-provider";

export type Offer = { title: string; artist: string; duration_ms: number };

export async function albumAudioProvider() {
  const audio = await readFile(
    "/workspace/fixtures/e2e-generated/Music/Fixture 440 Hz.wav",
  );
  const token = "browser-private-album-audio-token";
  const id = "e2e:audio/unidentified";
  const source = "e2e:audio-source/unidentified";
  const state: {
    outage: boolean;
    ready: boolean;
    offer: Offer;
    unexpected: number;
  } = {
    outage: false,
    ready: true,
    offer: {
      title: unidentified,
      artist: albumEdition.artistName,
      duration_ms: 1000,
    },
    unexpected: 0,
  };
  const calls: string[] = [];
  const audioRequests: string[] = [];
  const detail = () => ({
    id,
    ...state.offer,
    release: null,
    disc_number: null,
    track_number: null,
    revision: "album-audio-r1",
  });
  const server = createServer(async (request, response) => {
    const url = new URL(request.url!, "http://fixture.invalid");
    if (request.headers.authorization !== `Bearer ${token}`) {
      response.writeHead(401).end();
      return;
    }
    const route = url.pathname.replace(/^\/audio\//, "");
    if (route === "v1/audio") {
      audioRequests.push(url.searchParams.get("source_id") ?? "");
      serveAudio(request, response, audio);
      return;
    }
    let body = "";
    for await (const chunk of request) body += chunk;
    calls.push(route);
    const reply = (payload: unknown) =>
      response
        .writeHead(200, { "content-type": "application/json" })
        .end(JSON.stringify(payload));
    if (route === "v1/manifest")
      return reply({
        id: "org.hito.album-audio-e2e",
        name: "Album audio fixture",
        protocol_version: 1,
        capabilities: ["search", "sources", "metadata"],
      });
    if (state.outage) return response.writeHead(503).end();
    switch (route) {
      case "v1/search":
        return reply({ items: [detail()], next_cursor: null });
      case "v1/recordings/detail":
        return reply(detail());
      case "v1/recordings/metadata":
        return reply({
          recording: detail(),
          musical_title: state.offer.title,
          album_title: albumEdition.releaseTitle,
          source_ids: [source],
        });
      case "v1/sources":
        return reply({
          items: [{ id: source, label: "Original", ready: state.ready }],
        });
      default:
        state.unexpected++;
        response.writeHead(404).end();
    }
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  return {
    address: `http://127.0.0.1:${(server.address() as AddressInfo).port}/audio/`,
    token,
    calls,
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
