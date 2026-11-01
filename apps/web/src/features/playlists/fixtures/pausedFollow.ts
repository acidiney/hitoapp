import type { Schema } from "../../../shared/schema";
import { copiedPlaylistPage } from "./playlistPages";

export function followedPage(
  overrides: Partial<Schema<"PlaylistPageProps">> = {},
): Schema<"PlaylistPageProps"> {
  return {
    ...copiedPlaylistPage,
    detail: {
      ...copiedPlaylistPage.detail,
      playlist: { ...copiedPlaylistPage.detail.playlist, kind: "followed" },
      follow_content_revision: 1,
    },
    ...overrides,
  };
}

export function pausedFollow(
  playlist: string,
  generation: number,
  review: Partial<Schema<"PlaylistFollowReview">> = {},
): Schema<"PlaylistFollow"> {
  const now = Math.floor(Date.now() / 1000);
  return {
    playlist_id: playlist,
    generation,
    content_revision: 1,
    interval: "hourly",
    snapshot: "snapshot",
    last_refreshed_at: now,
    next_refresh_at: now + 3600,
    status: "paused",
    error: "authority_changed",
    review: {
      connection_name: "Spotify",
      installation_revision: 8,
      service_revision: 5,
      resumable: true,
      changes: [
        {
          kind: "capabilities",
          before: ["playlists"],
          after: ["playlists", "search"],
        },
      ],
      ...review,
    },
  };
}
