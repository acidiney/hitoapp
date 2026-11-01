import type { Schema } from "../../../shared/schema";
import { requestJson } from "../../../shared/http";

export const playlistDetailApi = {
  read(
    account: string,
    playlist: string,
    signal: AbortSignal,
  ): Promise<Schema<"PlaylistDetail">> {
    return requestJson<Schema<"PlaylistDetail">>(
      `/playlists/${encodeURIComponent(playlist)}`,
      { action: "playlists.detail", account, signal },
    );
  },
};
