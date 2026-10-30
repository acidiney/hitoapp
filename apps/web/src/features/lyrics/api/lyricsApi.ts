import type { Schema } from "../../../shared/schema";
import { requestJson } from "../../../shared/http";

export const lyricsApi = {
  async providers(
    account: string,
    recording: string,
    signal: AbortSignal,
  ): Promise<Schema<"RecordingLyricsProviders">> {
    return requestJson<Schema<"RecordingLyricsProviders">>(
      `/recordings/${encodeURIComponent(recording)}/lyrics/providers`,
      { action: "lyrics.providers", account, signal },
    );
  },
  async read(
    account: string,
    recording: string,
    body: Schema<"ReadRecordingLyrics">,
    signal: AbortSignal,
  ): Promise<Schema<"LyricsLookup">> {
    return requestJson<Schema<"LyricsLookup">>(
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
