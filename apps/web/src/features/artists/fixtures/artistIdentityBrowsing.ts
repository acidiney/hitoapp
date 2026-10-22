import type { components } from "../../../generated";
import { artistIdentityGroupDetail } from "./artistIdentity";
import {
  savedRecording,
  sourceAttachmentPage,
} from "../../sources/fixtures/sourceAttachment";

type Schema = components["schemas"];

export function artistIdentityBrowsing() {
  const group = {
    ...artistIdentityGroupDetail.group,
    library_count: 3,
    recording_count: 3,
    release_count: 3,
  };
  const index = {
    errors: {},
    native_playback: false,
    user: sourceAttachmentPage.user,
    playlists: sourceAttachmentPage.playlists,
    libraries: [
      { id: "owned-local-library", name: "Local library" },
      {
        id: savedRecording.library_id,
        name: "Imported library",
        personal: true,
      },
      { id: "other-library", name: "Another library" },
    ],
    query: group.name.value,
    artists: {
      items: [
        {
          id: "local-credit",
          library_id: "owned-local-library",
          name: group.name.value,
          recording_count: 2,
          release_count: 1,
          artwork_recording_id: null,
        },
        {
          id: "imported-credit",
          library_id: savedRecording.library_id,
          name: group.name.value,
          recording_count: 1,
          release_count: 1,
          artwork_recording_id: null,
        },
      ],
      total: 3,
      limit: 2,
      offset: 0,
    },
    identity_groups: {
      items: [
        group,
        {
          ...group,
          id: "other-chosen-group",
          provider_artist_id: "separate-provider-artist",
        },
      ],
      total: 3,
      limit: 2,
      offset: 0,
    },
  } satisfies Schema["ArtistIndexPageProps"];
  const localRecording = {
    ...savedRecording,
    id: "owned-local-recording",
    library_id: "owned-local-library",
    release_id: "owned-local-album",
    title: "Local recording title",
    inherited_title: "Local recording title",
    artist: "Local credit",
    inherited_artist: "Local credit",
    album: "Local album",
    available: true,
    liked: false,
  };
  const detail = {
    errors: index.errors,
    native_playback: index.native_playback,
    user: index.user,
    playlists: index.playlists,
    libraries: index.libraries,
    detail: {
      group,
      libraries: {
        items: [
          {
            library_id: localRecording.library_id,
            recording_count: 1,
            release_count: 1,
          },
          {
            library_id: savedRecording.library_id,
            recording_count: 1,
            release_count: 1,
          },
        ],
        total: 3,
        offset: 0,
        limit: 2,
      },
      recordings: {
        items: [
          {
            recording: localRecording,
            artist_id: "local-credit",
            association_id: "local-association",
            choice_revision: 1,
          },
          {
            ...artistIdentityGroupDetail.recordings.items[0],
            recording: { ...savedRecording },
          },
        ],
        total: 3,
        offset: 0,
        limit: 2,
      },
      releases: {
        items: [
          {
            id: localRecording.release_id,
            library_id: localRecording.library_id,
            title: localRecording.album,
            artist: null,
            artist_id: null,
            track_count: 1,
            duration_ms: localRecording.duration_ms,
          },
          {
            id: savedRecording.release_id!,
            library_id: savedRecording.library_id,
            title: savedRecording.album!,
            artist: "Saved album artist",
            artist_id: "imported-album-credit",
            track_count: 1,
            duration_ms: savedRecording.duration_ms,
          },
        ],
        total: 3,
        offset: 0,
        limit: 2,
      },
    },
  } satisfies Schema["ArtistIdentityGroupPageProps"];
  return { index, group, detail };
}
