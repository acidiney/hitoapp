import type { Schema } from "../../../shared/schema";
import { request } from "../../../shared/http";

import { availability } from "../../artists/api/albumEditionApi";

const album = (release: string) => `/albums/${encodeURIComponent(release)}`;

export const albumEditionChoiceApi = {
  async current(
    account: string,
    release: string,
    signal: AbortSignal,
  ): Promise<Schema<"AlbumEditionChoice">> {
    const response = await request(
      `/releases/${encodeURIComponent(release)}?limit=1`,
      {
        action: "album_edition.current",
        account,
        signal,
      },
    );
    const detail = (await response.json()) as Schema<"ReleaseDetail">;
    if (!detail.saved_edition)
      throw new Error("Hito returned an invalid album response. Try again.");
    return detail.saved_edition;
  },

  async choose(
    account: string,
    release: string,
    body: Schema<"SavedAlbumEdition"> & {
      request_id: string;
      expected_revision: number;
    },
    signal: AbortSignal,
  ): Promise<Schema<"AlbumEditionChoiceReceipt">> {
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
  ): Promise<Schema<"AlbumEditionChoiceReceipt">> {
    const response = await request(`${album(release)}/edition/clear`, {
      action: "album_edition.clear",
      method: "POST",
      account,
      body,
      signal,
    });
    return response.json();
  },

  async tagged(
    account: string,
    release: string,
    signal: AbortSignal,
  ): Promise<Schema<"TaggedAlbumEdition">> {
    const response = await request(`${album(release)}/tagged-edition`, {
      action: "album_edition.tagged",
      method: "POST",
      account,
      signal,
    });
    const value = (await response.json()) as Schema<"TaggedAlbumEdition">;
    if (value.status !== "found" && value.status !== "none")
      throw new Error("Hito returned an invalid edition response. Try again.");
    return value;
  },

  async tracks(
    account: string,
    release: string,
    signal: AbortSignal,
  ): Promise<Schema<"AlbumEditionTrackListAvailability">> {
    const response = await request(`${album(release)}/edition/track-list`, {
      action: "album_edition.saved_track_list",
      method: "POST",
      account,
      signal,
    });
    return availability(response);
  },
};
