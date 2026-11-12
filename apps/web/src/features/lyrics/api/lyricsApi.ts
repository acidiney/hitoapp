import type { Schema } from "../../../shared/schema";
import { requestJson, request } from "../../../shared/http";
import { invalidate } from "../../../shared/invalidation";

function base(recording: string): string {
  return `/recordings/${encodeURIComponent(recording)}/lyrics`;
}

export const lyricsApi = {
  async providers(
    account: string,
    recording: string,
    signal: AbortSignal,
  ): Promise<Schema<"RecordingLyricsProviders">> {
    return requestJson<Schema<"RecordingLyricsProviders">>(
      `${base(recording)}/providers`,
      { action: "lyrics.providers", account, signal },
    );
  },
  async read(
    account: string,
    recording: string,
    body: Schema<"ReadRecordingLyrics">,
    signal: AbortSignal,
  ): Promise<Schema<"LyricsLookup">> {
    return requestJson<Schema<"LyricsLookup">>(base(recording), {
      action: "lyrics.read",
      method: "POST",
      account,
      body,
      signal,
    });
  },
  async services(
    account: string,
    recording: string,
    signal: AbortSignal,
  ): Promise<Schema<"RecordingLyricServices">> {
    return requestJson<Schema<"RecordingLyricServices">>(
      `${base(recording)}/services`,
      { action: "lyrics.services", account, signal },
    );
  },
  async automatic(
    account: string,
    recording: string,
    signal: AbortSignal,
  ): Promise<Schema<"AutomaticLyricsLookup">> {
    const body: Schema<"ReadAutomaticRecordingLyrics"> = {};
    return requestJson<Schema<"AutomaticLyricsLookup">>(
      `${base(recording)}/automatic`,
      { action: "lyrics.automatic", method: "POST", account, body, signal },
    );
  },
  async identities(
    account: string,
    recording: string,
    signal: AbortSignal,
  ): Promise<Schema<"RecordingLyricIdentities">> {
    return requestJson<Schema<"RecordingLyricIdentities">>(
      `${base(recording)}/identities`,
      { action: "lyrics.identities", account, signal },
    );
  },
  async candidates(
    account: string,
    recording: string,
    body: Schema<"SearchRecordingLyrics">,
    signal: AbortSignal,
  ): Promise<Schema<"SearchPage">> {
    return requestJson<Schema<"SearchPage">>(`${base(recording)}/candidates`, {
      action: "lyrics.candidates",
      method: "POST",
      account,
      body,
      signal,
    });
  },
  async review(
    account: string,
    recording: string,
    body: Schema<"ProviderCandidate">,
    signal: AbortSignal,
  ): Promise<Schema<"RecordingLyricReview">> {
    return requestJson<Schema<"RecordingLyricReview">>(
      `${base(recording)}/identities/review`,
      { action: "lyrics.review", method: "POST", account, body, signal },
    );
  },
  async retain(
    account: string,
    recording: string,
    body: Schema<"RetainRecordingLyricIdentity">,
    signal: AbortSignal,
  ): Promise<Schema<"RecordingLyricIdentity">> {
    const retained = await requestJson<Schema<"RecordingLyricIdentity">>(
      `${base(recording)}/identities`,
      { action: "lyrics.retain", method: "POST", account, body, signal },
    );
    invalidate("lyrics", recording);
    return retained;
  },
  async readSelected(
    account: string,
    recording: string,
    body: Schema<"ReadSelectedRecordingLyrics">,
    signal: AbortSignal,
  ): Promise<Schema<"SelectedLyricsLookup">> {
    return requestJson<Schema<"SelectedLyricsLookup">>(
      `${base(recording)}/identities/read`,
      { action: "lyrics.read_selected", method: "POST", account, body, signal },
    );
  },
  async removeIdentity(
    account: string,
    recording: string,
    identity: { id: string; revision: number },
    signal: AbortSignal,
  ): Promise<void> {
    await request(
      `${base(recording)}/identities/${encodeURIComponent(identity.id)}?revision=${identity.revision}`,
      {
        action: "lyrics.remove_identity",
        method: "DELETE",
        account,
        signal,
      },
    );
    invalidate("lyrics", recording);
  },
};
