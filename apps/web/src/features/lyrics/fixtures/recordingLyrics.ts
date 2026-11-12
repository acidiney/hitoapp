import type { Schema } from "../../../shared/schema";

import { sourceAttachmentPage } from "../../sources/fixtures/sourceAttachment";

export const lyricObservation = {
  ...sourceAttachmentPage.detail.selected_observation!,
  id: "baf1d06d-4af7-4673-b76c-7fcdf5e05088",
  plugin_installation_id: "5feaf08b-2f32-4c0b-9b62-bcc6c18698dd",
  provider: "org.hito.owned-lyrics",
  provider_recording_id: 'opaque:lyrics/owned?reference="1"',
  provider_revision: 'owned-provider:"2"',
};

export const lyricRecordingPage: Schema<"RecordingDetailsPageProps"> = {
  ...sourceAttachmentPage,
  detail: {
    ...sourceAttachmentPage.detail,
    selected_observation: lyricObservation,
    observations: { items: [lyricObservation], next_before: null },
  },
};
