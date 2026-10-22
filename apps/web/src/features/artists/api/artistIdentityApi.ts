import { request } from "../../../shared/http";
import type {
  ArtistIdentityChoiceReceipt,
  ArtistIdentityGroupDetail,
  ChooseRecordingArtistIdentity,
  RecordingArtistIdentityChoice,
  ResetRecordingArtistIdentity,
} from "../../../shared/types";

const part = encodeURIComponent;

async function json<T>(
  path: string,
  account: string,
  signal: AbortSignal,
  body?: ChooseRecordingArtistIdentity | ResetRecordingArtistIdentity,
  method: "GET" | "PUT" | "POST" = "GET",
): Promise<T> {
  const response = await request(path, {
    action:
      method === "POST"
        ? "artist_identity.reset"
        : method === "PUT"
          ? "artist_identity.choose"
          : "artist_identity.read",
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
    return json<RecordingArtistIdentityChoice>(
      `/recordings/${part(recording)}/artist-identity`,
      account,
      signal,
    );
  },
  choose(
    account: string,
    recording: string,
    body: ChooseRecordingArtistIdentity,
    signal: AbortSignal,
  ) {
    return json<ArtistIdentityChoiceReceipt>(
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
    body: ResetRecordingArtistIdentity,
    signal: AbortSignal,
  ) {
    return json<ArtistIdentityChoiceReceipt>(
      `/recordings/${part(recording)}/artist-identity/reset`,
      account,
      signal,
      body,
      "POST",
    );
  },
  group(account: string, group: string, signal: AbortSignal) {
    return json<ArtistIdentityGroupDetail>(
      `/artist-identity-groups/${part(group)}?limit=1`,
      account,
      signal,
    );
  },
};
