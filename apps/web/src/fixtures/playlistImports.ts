import type { PlaylistIndexPageProps, PluginInstallation } from "../types";

export const playlistConnection: PluginInstallation = {
  id: "playlist-installation",
  service_id: "playlist-service",
  revision: 3,
  checked_at: 1,
  credential_set: false,
  manifest: {
    id: "org.hito.playlist-fixture",
    name: "Playlist fixture",
    protocol_version: 1,
    capabilities: ["playlists"],
  },
};

export const otherPlaylistConnection: PluginInstallation = {
  ...playlistConnection,
  id: "other-playlist-installation",
  service_id: "other-playlist-service",
  revision: 7,
};

export const playlistImportPage: PlaylistIndexPageProps = {
  user: { id: "alice", username: "Alice", admin: false },
  errors: {},
  native_playback: false,
  libraries: [],
  playlists: [],
  connections: [
    {
      ...playlistConnection,
      id: "search-installation",
      service_id: "search-service",
      manifest: { ...playlistConnection.manifest, capabilities: ["search"] },
    },
    playlistConnection,
  ],
  service_names: {
    "search-service": "Search collection",
    "playlist-service": "Playlist collection",
  },
};
