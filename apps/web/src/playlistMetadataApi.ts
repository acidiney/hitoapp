import { request } from "./http";
import type {
  ImportedPlaylistMetadataRefresh,
  RefreshImportedPlaylistMetadata,
} from "./types";

export const playlistMetadataApi = {
  async refresh(
    account: string,
    playlist: string,
    body: Readonly<RefreshImportedPlaylistMetadata>,
    signal: AbortSignal,
  ): Promise<ImportedPlaylistMetadataRefresh> {
    return (
      await request(
        `/playlists/${encodeURIComponent(playlist)}/metadata/refresh`,
        {
          action: "playlists.metadata.refresh",
          method: "POST",
          account,
          body,
          signal,
        },
      )
    ).json();
  },
};
