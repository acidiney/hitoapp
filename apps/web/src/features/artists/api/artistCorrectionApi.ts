import type { Schema } from "../../../shared/schema";
import { requestJson } from "../../../shared/http";

const part = encodeURIComponent;

export const artistCorrectionApi = {
  changes(account: string, group: string, signal: AbortSignal) {
    return requestJson<Schema<"ArtistIdentityGroupChanges">>(
      `/artist-identity-groups/${part(group)}/changes`,
      { action: "artist_identity.changes", account, signal },
    );
  },
  groups(account: string, signal: AbortSignal, search = "", offset = 0) {
    const query = new URLSearchParams({ limit: "20", offset: String(offset) });
    if (search) query.set("q", search);
    return requestJson<Schema<"ArtistIdentityGroupPage">>(
      `/artist-identity-groups?${query}`,
      { action: "artist_identity.groups", account, signal },
    );
  },
  split(
    account: string,
    group: string,
    body: Schema<"SplitArtistIdentityGroup">,
    signal: AbortSignal,
  ) {
    return requestJson<Schema<"ArtistIdentityChangeReceipt">>(
      `/artist-identity-groups/${part(group)}/split`,
      {
        action: "artist_identity.split",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
  undo(
    account: string,
    group: string,
    body: Schema<"UndoArtistIdentityChange">,
    signal: AbortSignal,
  ) {
    return requestJson<Schema<"ArtistIdentityChangeReceipt">>(
      `/artist-identity-groups/${part(group)}/undo`,
      {
        action: "artist_identity.undo",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
  shared(account: string, signal: AbortSignal, offset = 0, limit = 25) {
    return requestJson<Schema<"SharedArtistCorrections">>(
      `/artist-corrections?offset=${offset}&limit=${limit}`,
      { action: "artist_correction.list", account, signal },
    );
  },
  promote(
    account: string,
    body: Schema<"PromoteArtistCorrection">,
    signal: AbortSignal,
  ) {
    return requestJson<Schema<"SharedArtistCorrectionReceipt">>(
      "/artist-corrections/promote",
      {
        action: "artist_correction.promote",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
  undoShared(
    account: string,
    correction: string,
    body: Schema<"UndoSharedArtistCorrection">,
    signal: AbortSignal,
  ) {
    return requestJson<Schema<"SharedArtistCorrectionReceipt">>(
      `/artist-corrections/${part(correction)}/undo`,
      {
        action: "artist_correction.undo",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
};
