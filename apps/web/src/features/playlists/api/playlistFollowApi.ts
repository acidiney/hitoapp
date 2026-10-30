import type { Schema } from "../../../shared/schema";
import { requestJson } from "../../../shared/http";

const part = encodeURIComponent;

export const playlistFollowApi = {
  async status(
    account: string,
    playlist: string,
    signal: AbortSignal,
  ): Promise<Schema<"PlaylistFollow">> {
    return requestJson<Schema<"PlaylistFollow">>(
      `/playlists/${part(playlist)}/follow`,
      {
        action: "playlists.follow.status",
        account,
        signal,
      },
    );
  },
  async refresh(
    account: string,
    playlist: string,
    body: Readonly<Schema<"RefreshFollowedPlaylist">>,
    signal: AbortSignal,
  ): Promise<Schema<"PlaylistFollowRefresh">> {
    return requestJson<Schema<"PlaylistFollowRefresh">>(
      `/playlists/${part(playlist)}/follow/refresh`,
      {
        action: "playlists.follow.refresh",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
};
