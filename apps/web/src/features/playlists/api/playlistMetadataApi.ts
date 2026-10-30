import type { Schema } from "../../../shared/schema";
import { requestJson } from "../../../shared/http";

export const playlistMetadataApi = {
  async refresh(
    account: string,
    playlist: string,
    body: Readonly<Schema<"RefreshImportedPlaylistMetadata">>,
    signal: AbortSignal,
  ): Promise<Schema<"ImportedPlaylistMetadataRefresh">> {
    return requestJson<Schema<"ImportedPlaylistMetadataRefresh">>(
      `/playlists/${encodeURIComponent(playlist)}/metadata/refresh`,
      {
        action: "playlists.metadata.refresh",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
};
