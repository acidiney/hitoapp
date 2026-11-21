import type { Schema } from "../../../shared/schema";
import { requestJson } from "../../../shared/http";

export const albumAdditionApi = {
  add(
    account: string,
    release: string,
    body: Schema<"AddAlbum">,
    signal: AbortSignal,
  ): Promise<Schema<"AlbumAddition">> {
    return requestJson<Schema<"AlbumAddition">>(
      `/albums/${encodeURIComponent(release)}/additions`,
      { action: "album_addition.add", method: "POST", account, body, signal },
    );
  },
  addFromArtist(
    account: string,
    recording: string,
    association: string,
    body: Schema<"AddArtistAlbum">,
    signal: AbortSignal,
  ): Promise<Schema<"AlbumAddition">> {
    return requestJson<Schema<"AlbumAddition">>(
      `/recordings/${encodeURIComponent(recording)}/artist-associations/${encodeURIComponent(association)}/additions`,
      { action: "album_addition.add", method: "POST", account, body, signal },
    );
  },
  undo(
    account: string,
    addition: string,
    signal: AbortSignal,
  ): Promise<Schema<"AlbumAdditionUndone">> {
    return requestJson<Schema<"AlbumAdditionUndone">>(
      `/albums/additions/${encodeURIComponent(addition)}`,
      { action: "album_addition.undo", method: "DELETE", account, signal },
    );
  },
};
