import type { Schema } from "../../../shared/schema";
import { requestJson } from "../../../shared/http";

const part = encodeURIComponent;

export const artistAssociationApi = {
  changes(account: string, recording: string, signal: AbortSignal) {
    return requestJson<Schema<"ArtistAssociationChanges">>(
      `/recordings/${part(recording)}/artist-association-changes`,
      { action: "artist_association.changes", account, signal },
    );
  },
  unlink(
    account: string,
    recording: string,
    body: Schema<"UnlinkRecordingArtist">,
    signal: AbortSignal,
  ) {
    return requestJson<Schema<"ArtistAssociationChangeReceipt">>(
      `/recordings/${part(recording)}/artist-associations/unlink`,
      {
        action: "artist_association.unlink",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
  undo(
    account: string,
    recording: string,
    body: Schema<"UndoRecordingArtistChange">,
    signal: AbortSignal,
  ) {
    return requestJson<Schema<"ArtistAssociationChangeReceipt">>(
      `/recordings/${part(recording)}/artist-associations/undo`,
      {
        action: "artist_association.undo",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
};
