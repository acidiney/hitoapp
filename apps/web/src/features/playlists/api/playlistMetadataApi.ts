import { requestJson } from "../../../shared/http";
import type {
  ImportedPlaylistMetadataRefresh,
  RefreshImportedPlaylistMetadata,
} from "../../../shared/types";

export const playlistMetadataApi = {
  async refresh(
    account: string,
    playlist: string,
    body: Readonly<RefreshImportedPlaylistMetadata>,
    signal: AbortSignal,
  ): Promise<ImportedPlaylistMetadataRefresh> {
    return requestJson<ImportedPlaylistMetadataRefresh>(
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
