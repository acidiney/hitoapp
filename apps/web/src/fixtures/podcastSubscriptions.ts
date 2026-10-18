import type { components } from "../generated";

export const ownedPodcastFeed =
  "https://feed.invalid/owned-private-existing-podcast.xml";

export const ownedPodcastJob = "f56fc4c5-8e86-4afd-9db2-356233bb79b2";

export const readyPodcast = {
  subscription_id: "515993a6-776f-42bc-9d28-d4df99ceae28",
  title: "Owned private existing podcast",
  refresh_pending: false,
  refresh_failed: false,
  last_refreshed_at: 1_790_000_000,
  content_revision: 1,
} satisfies components["schemas"]["PodcastSubscription"];

export const emptyPodcastPage = {
  user: { id: "alice", username: "Alice", admin: false },
  errors: {},
  native_playback: false,
  libraries: [],
  playlists: [],
  subscriptions: [],
} satisfies components["schemas"]["PodcastIndexPageProps"];
