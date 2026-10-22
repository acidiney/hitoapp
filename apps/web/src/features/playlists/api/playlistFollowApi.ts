import { requestJson } from "../../../shared/http";
import type {
  PlaylistFollow,
  PlaylistFollowRefresh,
  RefreshFollowedPlaylist,
} from "../../../shared/types";

const part = encodeURIComponent;

export const playlistFollowApi = {
  async status(
    account: string,
    playlist: string,
    signal: AbortSignal,
  ): Promise<PlaylistFollow> {
    return requestJson<PlaylistFollow>(`/playlists/${part(playlist)}/follow`, {
      action: "playlists.follow.status",
      account,
      signal,
    });
  },
  async refresh(
    account: string,
    playlist: string,
    body: Readonly<RefreshFollowedPlaylist>,
    signal: AbortSignal,
  ): Promise<PlaylistFollowRefresh> {
    return requestJson<PlaylistFollowRefresh>(
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
