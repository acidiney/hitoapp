import type { Schema } from "../../../shared/schema";
import { request } from "../../../shared/http";

const part = encodeURIComponent;

async function json<T>(
  path: string,
  account: string,
  signal: AbortSignal,
  body?:
    | Schema<"ChooseRecordingArtistIdentity">
    | Schema<"ResetRecordingArtistIdentity">
    | Schema<"ApplyArtistIdentityToCredit">,
  method: "GET" | "PUT" | "POST" = "GET",
  action?: string,
): Promise<T> {
  const response = await request(path, {
    action:
      action ??
      (method === "POST"
        ? "artist_identity.reset"
        : method === "PUT"
          ? "artist_identity.choose"
          : "artist_identity.read"),
    account,
    method,
    body,
    signal,
  });
  if (signal.aborted) {
    await response.body?.cancel();
    signal.throwIfAborted();
  }
  return response.json();
}

export const artistIdentityApi = {
  read(account: string, recording: string, signal: AbortSignal) {
    return json<Schema<"RecordingArtistIdentityChoice">>(
      `/recordings/${part(recording)}/artist-identity`,
      account,
      signal,
    );
  },
  choose(
    account: string,
    recording: string,
    body: Schema<"ChooseRecordingArtistIdentity">,
    signal: AbortSignal,
  ) {
    return json<Schema<"ArtistIdentityChoiceReceipt">>(
      `/recordings/${part(recording)}/artist-identity`,
      account,
      signal,
      body,
      "PUT",
    );
  },
  reset(
    account: string,
    recording: string,
    body: Schema<"ResetRecordingArtistIdentity">,
    signal: AbortSignal,
  ) {
    return json<Schema<"ArtistIdentityChoiceReceipt">>(
      `/recordings/${part(recording)}/artist-identity/reset`,
      account,
      signal,
      body,
      "POST",
    );
  },
  group(account: string, group: string, signal: AbortSignal) {
    return json<Schema<"ArtistIdentityGroupDetail">>(
      `/artist-identity-groups/${part(group)}?limit=1`,
      account,
      signal,
    );
  },
  groupsForCredit(account: string, credit: string, signal: AbortSignal) {
    const query = new URLSearchParams({ q: credit, limit: "20" });
    return json<Schema<"ArtistIdentityGroupPage">>(
      `/artist-identity-groups?${query}`,
      account,
      signal,
    );
  },
  applyToCredit(
    account: string,
    artist: string,
    body: Schema<"ApplyArtistIdentityToCredit">,
    signal: AbortSignal,
  ) {
    return json<Schema<"ArtistIdentityCreditReceipt">>(
      `/artists/${part(artist)}/artist-identity`,
      account,
      signal,
      body,
      "POST",
      "artist_identity.apply_to_credit",
    );
  },
};
