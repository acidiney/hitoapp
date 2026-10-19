import { request } from "./http";
import type { components } from "./generated";

export type LyricsLookup = components["schemas"]["LyricsLookup"];
export type ReadRecordingLyrics = components["schemas"]["ReadRecordingLyrics"];

export const lyricsApi = {
  async read(
    account: string,
    recording: string,
    body: ReadRecordingLyrics,
    signal: AbortSignal,
  ): Promise<LyricsLookup> {
    return (
      await request(`/recordings/${encodeURIComponent(recording)}/lyrics`, {
        action: "lyrics.read",
        method: "POST",
        account,
        body,
        signal,
      })
    ).json();
  },
};
