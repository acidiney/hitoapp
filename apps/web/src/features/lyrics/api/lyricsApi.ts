import { request } from "../../../shared/http";
import type { components } from "../../../generated";

export type LyricsLookup = components["schemas"]["LyricsLookup"];
export type ReadRecordingLyrics = components["schemas"]["ReadRecordingLyrics"];
export type RecordingLyricsProvider =
  components["schemas"]["RecordingLyricsProvider"];
export type RecordingLyricsProviders =
  components["schemas"]["RecordingLyricsProviders"];

export const lyricsApi = {
  async providers(
    account: string,
    recording: string,
    signal: AbortSignal,
  ): Promise<RecordingLyricsProviders> {
    return (
      await request(
        `/recordings/${encodeURIComponent(recording)}/lyrics/providers`,
        { action: "lyrics.providers", account, signal },
      )
    ).json();
  },
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
