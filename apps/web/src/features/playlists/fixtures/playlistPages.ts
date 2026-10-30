import type { Schema } from "../../../shared/schema";

export const recordingA = "cd53993a-c582-42af-a67b-61280e39d3c2";
export const recordingB = "eb6a9e9e-39fc-464f-bb2a-3ae17834e748";
const first: Schema<"PlaylistEntry"> = {
  entry_id: "cf6d6fcb-1644-44b4-b49d-8ebd81e67250",
  recording_id: recordingA,
  position: 0,
  title: "Fixture A",
  inherited_title: "Fixture A",
  artist: "Fixture artist",
  inherited_artist: "Fixture artist",
  duration_ms: 12000,
  liked: false,
  available: true,
  origin: {
    kind: "recording",
    provider_recording_id: "provider/a",
    occurrence_id: "first-a",
  },
};
export const unknown: Schema<"PlaylistEntry"> = {
  ...first,
  entry_id: "dd56c1d9-4738-4993-8cfc-76a65cabf77d",
  recording_id: null,
  position: 1,
  title: "Unknown provider item",
  inherited_title: "Unknown provider item",
  artist: "",
  inherited_artist: "",
  duration_ms: 0,
  available: false,
  origin: {
    kind: "unknown",
    provider_recording_id: null,
    occurrence_id: "unknown",
  },
};
export const copiedPlaylistPage: Schema<"PlaylistPageProps"> = {
  user: { id: "alice", username: "Alice", admin: false },
  errors: {},
  native_playback: false,
  lock: { pin_enabled: false, idle_minutes: 0, locked: false },
  libraries: [],
  playlists: [],
  detail: {
    playlist: {
      id: "eb0f0af1-dc27-4737-ad8a-928b8bf6a32e",
      name: "Copied selection",
      kind: "local",
    },
    origin: {
      provider: "org.hito.playlist-fixture",
      installation_id: "removed-installation",
      installation_revision: 3,
      service_id: "removed-service",
      service_revision: 1,
      provider_playlist_id: "canonical/selection",
      snapshot: "retained-snapshot",
      imported_at: 1,
      attribution: { label: "Curated by the fixture owner" },
    },
    entries: [
      first,
      unknown,
      {
        ...first,
        entry_id: "1813dcbd-9ce3-4b9d-bfb6-c70d3f4a564f",
        recording_id: recordingB,
        position: 2,
        title: "Fixture B",
        inherited_title: "Fixture B",
        origin: {
          kind: "recording",
          provider_recording_id: "provider/b",
          occurrence_id: "b",
        },
      },
      {
        ...first,
        entry_id: "b3ecda1c-e351-42d5-8916-0d0eb32ae081",
        position: 3,
        origin: {
          kind: "recording",
          provider_recording_id: "provider/a",
          occurrence_id: "last-a",
        },
      },
    ],
  },
};

export const unavailablePlaylistPages: Schema<"PlaylistPageProps">[] = [
  {
    ...copiedPlaylistPage,
    detail: {
      ...copiedPlaylistPage.detail,
      entries: [
        { ...unknown, position: 0 },
        {
          ...unknown,
          entry_id: "aee3babd-2bde-487a-8c67-bd76b08197ca",
          position: 1,
          title: "Withdrawn provider item",
          inherited_title: "Withdrawn provider item",
          artist: "Retained artist",
          inherited_artist: "Retained artist",
          origin: {
            kind: "unavailable",
            provider_recording_id: "provider/withdrawn",
            occurrence_id: "withdrawn",
          },
        },
      ],
    },
  },
  {
    ...copiedPlaylistPage,
    detail: {
      ...copiedPlaylistPage.detail,
      entries: [first, copiedPlaylistPage.detail.entries[2]].map(
        (entry, index) => ({
          ...entry,
          position: index,
          available: false,
        }),
      ),
    },
  },
];
