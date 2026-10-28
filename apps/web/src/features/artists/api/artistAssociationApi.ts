import { requestJson } from "../../../shared/http";
import type {
  ArtistAssociationChangeReceipt,
  ArtistAssociationChanges,
  UndoRecordingArtistChange,
  UnlinkRecordingArtist,
} from "../../../shared/types";

const part = encodeURIComponent;

export const artistAssociationApi = {
  changes(account: string, recording: string, signal: AbortSignal) {
    return requestJson<ArtistAssociationChanges>(
      `/recordings/${part(recording)}/artist-association-changes`,
      { action: "artist_association.changes", account, signal },
    );
  },
  unlink(
    account: string,
    recording: string,
    body: UnlinkRecordingArtist,
    signal: AbortSignal,
  ) {
    return requestJson<ArtistAssociationChangeReceipt>(
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
    body: UndoRecordingArtistChange,
    signal: AbortSignal,
  ) {
    return requestJson<ArtistAssociationChangeReceipt>(
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
