import { requestJson } from "../../../shared/http";
import type {
  LyricsLookup,
  ReadRecordingLyrics,
  RecordingLyricsProviders,
} from "../../../shared/types";

export const lyricsApi = {
  async providers(
    account: string,
    recording: string,
    signal: AbortSignal,
  ): Promise<RecordingLyricsProviders> {
    return requestJson<RecordingLyricsProviders>(
      `/recordings/${encodeURIComponent(recording)}/lyrics/providers`,
      { action: "lyrics.providers", account, signal },
    );
  },
  async read(
    account: string,
    recording: string,
    body: ReadRecordingLyrics,
    signal: AbortSignal,
  ): Promise<LyricsLookup> {
    return requestJson<LyricsLookup>(
      `/recordings/${encodeURIComponent(recording)}/lyrics`,
      {
        action: "lyrics.read",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
};
