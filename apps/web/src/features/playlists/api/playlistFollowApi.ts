import { request } from "../../../shared/http";
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
    return (
      await request(`/playlists/${part(playlist)}/follow`, {
        action: "playlists.follow.status",
        account,
        signal,
      })
    ).json();
  },
  async refresh(
    account: string,
    playlist: string,
    body: Readonly<RefreshFollowedPlaylist>,
    signal: AbortSignal,
  ): Promise<PlaylistFollowRefresh> {
    return (
      await request(`/playlists/${part(playlist)}/follow/refresh`, {
        action: "playlists.follow.refresh",
        method: "POST",
        account,
        body,
        signal,
      })
    ).json();
  },
};
