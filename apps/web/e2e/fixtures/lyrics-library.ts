import { expect, type APIRequestContext, type Page } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";
import {
  connectLyrics,
  lyricsProvider,
  type LyricsCandidate,
} from "./lyrics-provider";

type Facts = Pick<LyricsCandidate, "title" | "artist" | "duration_ms">;

export async function libraryTrack(page: Page, title: string) {
  const listed = await page.request.get(
    `/api/v1/recordings?q=${encodeURIComponent(title)}`,
  );
  const body = await listed.json();
  const recording = body.items?.find(
    (item: Schema<"Recording">) => item.title === title,
  ) as Schema<"Recording">;
  if (!recording) throw new Error("The scanned fixture library is required");
  const facts: Facts = {
    title: recording.title,
    artist: recording.artist,
    duration_ms: recording.duration_ms!,
  };
  return { id: recording.id, facts };
}

export async function withProvider(
  page: Page,
  run: (
    provider: Awaited<ReturnType<typeof lyricsProvider>>,
    connection: Awaited<ReturnType<typeof connectLyrics>>,
  ) => Promise<void>,
  baseURL: string,
  administrator?: APIRequestContext,
) {
  const provider = await lyricsProvider();
  const connection = await connectLyrics(
    page.request,
    new URL(baseURL).origin,
    provider,
    administrator,
  );
  try {
    await run(provider, connection);
    expect(provider.state.unexpected).toBe(0);
  } finally {
    provider.server.stage("removing-connection");
    await connection.remove();
    provider.server.stage("connection-removed");
    await provider.close();
  }
}
