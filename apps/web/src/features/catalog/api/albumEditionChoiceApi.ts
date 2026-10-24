import { request } from "../../../shared/http";
import type {
  AlbumEditionChoiceReceipt,
  AlbumEditionTrackListAvailability,
  SavedAlbumEdition,
} from "../../../shared/types";
import { availability } from "../../artists/api/albumEditionApi";

const album = (release: string) => `/albums/${encodeURIComponent(release)}`;

export const albumEditionChoiceApi = {
  async choose(
    account: string,
    release: string,
    body: SavedAlbumEdition & { request_id: string; expected_revision: number },
    signal: AbortSignal,
  ): Promise<AlbumEditionChoiceReceipt> {
    const response = await request(`${album(release)}/edition`, {
      action: "album_edition.choose",
      method: "PUT",
      account,
      body,
      signal,
    });
    return response.json();
  },

  async clear(
    account: string,
    release: string,
    body: { request_id: string; expected_revision: number },
    signal: AbortSignal,
  ): Promise<AlbumEditionChoiceReceipt> {
    const response = await request(`${album(release)}/edition/clear`, {
      action: "album_edition.clear",
      method: "POST",
      account,
      body,
      signal,
    });
    return response.json();
  },

  async tracks(
    account: string,
    release: string,
    signal: AbortSignal,
  ): Promise<AlbumEditionTrackListAvailability> {
    const response = await request(`${album(release)}/edition/track-list`, {
      action: "album_edition.saved_track_list",
      method: "POST",
      account,
      signal,
    });
    return availability(response);
  },
};
