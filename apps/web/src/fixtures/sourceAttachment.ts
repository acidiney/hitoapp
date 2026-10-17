import type { components } from "../generated";
import type {
  MetadataObservation,
  PluginSettings,
  ProviderSearchPage,
  Recording,
  RecordingDetailsPageProps,
  Source,
} from "../types";
import { copiedPlaylistPage, recordingA } from "./playlistPages";

type Schema = components["schemas"];

export const savedRecording: Recording = {
  id: recordingA,
  library_id: "055e4b7b-6062-4d23-bba5-01dbe4612d71",
  release_id: "c7cbe56d-3f07-41ed-bc46-59685a1cffc8",
  title: "My saved title",
  inherited_title: "Spotify title",
  artist: "My saved artist",
  inherited_artist: "Spotify artist",
  album: "Spotify album",
  duration_ms: 120_000,
  liked: true,
  available: false,
};

const primary: MetadataObservation = {
  id: "674178dd-357b-49f4-b780-a050b77c4499",
  source_id: null,
  release_id: savedRecording.release_id ?? null,
  provider: "org.hito.spotify-fixture",
  origin: "provider_response",
  observed_at: 1,
  inspection_version: null,
  plugin_installation_id: "disconnected-spotify",
  provider_recording_id: "spotify/saved",
  provider_revision: "spotify-v1",
  title: savedRecording.inherited_title,
  artist: savedRecording.inherited_artist,
  album: savedRecording.album ?? null,
  album_artist: "Spotify artist",
  duration_ms: savedRecording.duration_ms,
  disc_number: null,
  track_number: null,
  tags: null,
};

export const sourceAttachmentPage: RecordingDetailsPageProps = {
  user: copiedPlaylistPage.user,
  errors: {},
  native_playback: false,
  libraries: [],
  playlists: [copiedPlaylistPage.detail.playlist],
  detail: {
    recording_id: savedRecording.id,
    title: {
      value: savedRecording.title,
      personal: savedRecording.title,
      inherited: savedRecording.inherited_title,
      operation_id: "963dc7c3-f87f-498d-9f8b-42276fb7d151",
    },
    artist: {
      value: savedRecording.artist,
      personal: savedRecording.artist,
      inherited: savedRecording.inherited_artist,
      operation_id: "3c878f96-8a55-4164-a877-7a65c1c88e88",
    },
    selected_observation: primary,
    observations: { items: [primary], next_before: null },
  },
};

export const attachmentSettings: PluginSettings = {
  credential_storage_ready: true,
  services: [
    {
      id: "video-service",
      name: "Video archive",
      base_url: "http://owned-video.invalid/",
      revision: 1,
    },
    {
      id: "search-service",
      name: "Search catalogue",
      base_url: "http://owned-search.invalid/",
      revision: 1,
    },
    {
      id: "metadata-service",
      name: "Metadata catalogue",
      base_url: "http://owned-metadata.invalid/",
      revision: 1,
    },
  ],
  installations: [
    {
      id: "video-connection",
      service_id: "video-service",
      revision: 7,
      checked_at: 1,
      credential_set: false,
      manifest: {
        id: "org.hito.youtube-fixture",
        name: "Owned videos",
        protocol_version: 1,
        capabilities: ["search", "sources"],
      },
    },
    {
      id: "search-connection",
      service_id: "search-service",
      revision: 1,
      checked_at: 1,
      credential_set: false,
      manifest: {
        id: "org.hito.search-fixture",
        name: "Search catalogue",
        protocol_version: 1,
        capabilities: ["search"],
      },
    },
    {
      id: "metadata-connection",
      service_id: "metadata-service",
      revision: 1,
      checked_at: 1,
      credential_set: false,
      manifest: {
        id: "org.hito.metadata-fixture",
        name: "Metadata catalogue",
        protocol_version: 1,
        capabilities: ["playlists"],
      },
    },
  ],
};

export const attachmentCandidate: Schema["ProviderCandidate"] = {
  installation_id: "video-connection",
  installation_revision: 7,
  provider_recording_id: "video/candidate",
};

export const attachmentSearch: ProviderSearchPage = {
  items: [
    {
      id: "video/candidate",
      title: "Search result title",
      artist: "Search result artist",
      duration_ms: 121_000,
    },
  ],
  next_cursor: null,
};

export const attachmentReview: Schema["ProviderSourceReview"] = {
  candidate: {
    id: "video/candidate",
    title: "Reviewed video title",
    artist: "Reviewed video artist",
    duration_ms: 124_000,
  },
  review_stamp: "provider-source-v1:owned-review-stamp",
};

export const reviewedAttachment: Schema["AttachReviewedProviderSource"] = {
  candidate: attachmentCandidate,
  review_stamp: attachmentReview.review_stamp,
};

export const attachedRecording: Recording = {
  ...savedRecording,
  available: true,
};

export const attachedSources: Source[] = [
  {
    id: "accepted-video-audio",
    recording_id: savedRecording.id,
    provider: "plugin",
    device_id: null,
    ready: true,
    label: "Owned video audio",
    can_prepare: false,
  },
];

export const attachedPage: RecordingDetailsPageProps = {
  ...sourceAttachmentPage,
  detail: {
    ...sourceAttachmentPage.detail,
    observations: {
      next_before: null,
      items: [
        {
          ...primary,
          id: "202ff026-9c26-40ce-98fd-b2e82f7745c7",
          source_id: attachedSources[0].id,
          release_id: null,
          provider: "org.hito.youtube-fixture",
          plugin_installation_id: attachmentCandidate.installation_id,
          provider_recording_id: attachmentCandidate.provider_recording_id,
          provider_revision: null,
          title: attachmentReview.candidate.title,
          artist: attachmentReview.candidate.artist,
          album: null,
          album_artist: null,
          duration_ms: attachmentReview.candidate.duration_ms ?? 0,
        },
        primary,
      ],
    },
  },
};
