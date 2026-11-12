import type { IncomingMessage, ServerResponse } from "node:http";
import type { APIRequestContext } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";
import { listenOwned } from "./owned-http-service";

export type LyricsContent =
  | { kind: "plain"; lines: string[] }
  | { kind: "anchored"; lines: { start_ms: number; text: string }[] };

export type LyricsCandidate = {
  id: string;
  title: string;
  artist: string;
  duration_ms: number;
  lyrics: LyricsContent | null | "fail";
};

export const plainLines = ["Plain river line", "Plain horizon line"];
export const timedLines = [
  { start_ms: 0, text: "Timed river line" },
  { start_ms: 30_000, text: "Timed horizon line" },
  { start_ms: 60_000, text: "Timed closing line" },
];

export function plainCandidate(
  id: string,
  facts: Pick<LyricsCandidate, "title" | "artist" | "duration_ms">,
): LyricsCandidate {
  return { id, ...facts, lyrics: { kind: "plain", lines: plainLines } };
}

export function timedCandidate(
  id: string,
  facts: Pick<LyricsCandidate, "title" | "artist" | "duration_ms">,
): LyricsCandidate {
  return { id, ...facts, lyrics: { kind: "anchored", lines: timedLines } };
}

export async function lyricsProvider() {
  const token = "browser-private-lyrics-token";
  const state: {
    candidates: LyricsCandidate[];
    listing: (query: string) => LyricsCandidate[];
    continuation: boolean;
    unexpected: number;
  } = {
    candidates: [],
    listing: () => state.candidates,
    continuation: false,
    unexpected: 0,
  };
  const calls: string[] = [];
  const abandoned: string[] = [];
  const holds = new Map<string, Promise<void>>();
  const releases = new Map<string, () => void>();
  const entered = new Map<string, () => void>();
  const arrivals = new Map<string, Promise<void>>();

  const hold = (id: string) => {
    holds.set(id, new Promise<void>((resolve) => releases.set(id, resolve)));
    arrivals.set(id, new Promise<void>((resolve) => entered.set(id, resolve)));
  };
  const release = (id: string) => {
    holds.delete(id);
    releases.get(id)?.();
  };
  const detail = (candidate: LyricsCandidate) => ({
    id: candidate.id,
    title: candidate.title,
    artist: candidate.artist,
    duration_ms: candidate.duration_ms,
    release: null,
    disc_number: null,
    track_number: null,
    revision: null,
  });
  const find = (id: string) => state.candidates.find((item) => item.id === id);

  async function handle(request: IncomingMessage, response: ServerResponse) {
    const url = new URL(request.url!, "http://fixture.invalid");
    if (request.headers.authorization !== `Bearer ${token}`) {
      response.writeHead(401).end();
      return;
    }
    const route = url.pathname.replace(/^\/lyrics\//, "");
    let text = "";
    for await (const chunk of request) text += chunk;
    const body = text ? JSON.parse(text) : {};
    const reply = (payload: unknown) =>
      response
        .writeHead(200, { "content-type": "application/json" })
        .end(JSON.stringify(payload));
    if (route === "v1/manifest")
      return reply({
        id: "org.hito.lyrics-e2e",
        name: "Lyrics fixture",
        protocol_version: 1,
        capabilities: ["search", "metadata", "lyrics"],
      });
    if (route === "v2/lyrics/capabilities") return reply({ lyrics_version: 2 });
    if (route === "v1/search") {
      calls.push("search");
      return reply({
        items: state.listing(String(body.query)).map(detail),
        next_cursor: state.continuation ? "more" : null,
      });
    }
    if (
      route === "v1/recordings/detail" ||
      route === "v1/recordings/metadata"
    ) {
      const candidate = find(String(body.recording_id));
      calls.push(`metadata:${body.recording_id}`);
      if (!candidate) return response.writeHead(404).end();
      return route === "v1/recordings/detail"
        ? reply(detail(candidate))
        : reply({
            recording: detail(candidate),
            musical_title: candidate.title,
          });
    }
    if (route === "v2/lyrics") {
      const id = String(body.recording?.id);
      calls.push(`lyrics:${id}`);
      const candidate = find(id);
      if (!candidate) return response.writeHead(404).end();
      response.on("close", () => {
        if (!response.writableFinished) abandoned.push(id);
      });
      const pending = holds.get(id);
      if (pending) {
        entered.get(id)?.();
        await pending;
      }
      if (candidate.lyrics === "fail") return response.writeHead(503).end();
      return reply({
        recording: { id, revision: null },
        lyrics:
          candidate.lyrics === null
            ? null
            : {
                id: `doc-${id}`,
                rendition_id: `rendition-${id}`,
                revision: null,
                rendition_label: null,
                language: null,
                duration_ms:
                  candidate.lyrics.kind === "anchored"
                    ? candidate.duration_ms
                    : null,
                attribution: null,
                content: candidate.lyrics,
              },
      });
    }
    state.unexpected++;
    response.writeHead(404).end();
  }

  const server = await listenOwned("lyrics-provider", (request, response) => {
    void handle(request, response).catch(() => {
      state.unexpected++;
      if (!response.headersSent) response.writeHead(500);
      response.end();
    });
  });
  return {
    address: `http://127.0.0.1:${server.port}/lyrics/`,
    server,
    token,
    state,
    calls,
    abandoned,
    hold,
    release,
    held: (id: string) => arrivals.get(id)!,
    reads: () => calls.filter((call) => call.startsWith("lyrics:")),
    close: async () => {
      for (const id of [...holds.keys()]) release(id);
      await server.close(10000);
    },
  };
}

export async function connectLyrics(
  request: APIRequestContext,
  origin: string,
  provider: Awaited<ReturnType<typeof lyricsProvider>>,
  administrator: APIRequestContext = request,
) {
  const headers = { "x-hito-request": "1", origin };
  const offered = await administrator.post("/api/v1/plugins/services", {
    headers,
    data: {
      name: "Lyrics fixture",
      base_url: provider.address,
    } satisfies Schema<"OfferPluginService">,
  });
  if (offered.status() !== 201)
    throw new Error("The lyrics service was refused");
  const service = (await offered.json()) as Schema<"PluginService">;
  const installed = await request.post("/api/v1/plugins", {
    headers,
    data: {
      service_id: service.id,
      service_revision: service.revision,
      bearer_token: provider.token,
    } satisfies Schema<"InstallPlugin">,
  });
  if (installed.status() !== 201)
    throw new Error("The lyrics connection was refused");
  const connection = (await installed.json()) as Schema<"PluginInstallation">;
  return {
    connection,
    remove: async () => {
      await request.delete(`/api/v1/plugins/${connection.id}`, {
        headers,
        data: { revision: connection.revision },
      });
    },
  };
}
