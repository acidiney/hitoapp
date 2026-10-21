import type { components } from "../generated";
import type { PluginSettings, RecordingDetailsPageProps } from "../types";
import { sourceAttachmentPage } from "./sourceAttachment";

type Schema = components["schemas"];

export const lyricObservation = {
  ...sourceAttachmentPage.detail.selected_observation!,
  id: "baf1d06d-4af7-4673-b76c-7fcdf5e05088",
  plugin_installation_id: "5feaf08b-2f32-4c0b-9b62-bcc6c18698dd",
  provider: "org.hito.owned-lyrics",
  provider_recording_id: 'opaque:lyrics/owned?reference="1"',
  provider_revision: 'owned-provider:"2"',
};

export const lyricRecordingPage: RecordingDetailsPageProps = {
  ...sourceAttachmentPage,
  detail: {
    ...sourceAttachmentPage.detail,
    selected_observation: lyricObservation,
    observations: { items: [lyricObservation], next_before: null },
  },
};

export const lyricSettings: PluginSettings = {
  credential_storage_ready: true,
  services: [
    {
      id: "96974ea2-6352-4355-94db-ddb5a25f4a43",
      name: "Owned private lyric provider",
      base_url: "https://private-lyric-provider.invalid/service/",
      revision: 4,
    },
  ],
  installations: [
    {
      id: "8e822ec7-42ed-42a0-84db-f5a6dc682ca7",
      service_id: "96974ea2-6352-4355-94db-ddb5a25f4a43",
      checked_at: 1,
      credential_set: false,
      revision: 2,
      manifest: {
        id: "org.hito.other-lyrics",
        name: "Other lyrics",
        protocol_version: 1,
        capabilities: ["lyrics"],
      },
    },
    {
      id: lyricObservation.plugin_installation_id,
      service_id: "96974ea2-6352-4355-94db-ddb5a25f4a43",
      checked_at: 1,
      credential_set: true,
      revision: 9,
      manifest: {
        id: lyricObservation.provider,
        name: "Owned private lyrics",
        protocol_version: 1,
        capabilities: ["playlists", "lyrics"],
      },
    },
  ],
};

export const readyLyrics = {
  status: "ready",
  recording_id: lyricRecordingPage.detail.recording_id,
  observation_id: lyricObservation.id,
  document_digest:
    "060d07c159e679c9c9d7e4fe49196858693125773545272e4f5689f15ef030c5",
  document: {
    recording: {
      id: lyricObservation.provider_recording_id,
      revision: lyricObservation.provider_revision,
    },
    lyrics: {
      id: 'private:lyrics/document?edition="3"',
      rendition_id: "private:lyrics/rendition",
      revision: 'owned-document:"3"',
      rendition_label: "Owned lyric rendition",
      language: "en",
      duration_ms: 2_000,
      attribution: {
        creators: ["Owned lyric creator"],
        provider_label: "Owned lyric credits",
        content_url: "https://lyrics.invalid/credits/owned-recording",
      },
      lines: [
        {
          start_ms: 0,
          end_ms: 1_000,
          text: '<strong>Owned private "lyric" line</strong>',
        },
        {
          start_ms: 1_000,
          end_ms: 2_000,
          text: "Owned second static lyric line",
        },
      ],
    },
  },
} satisfies Schema["LyricsLookup"];
