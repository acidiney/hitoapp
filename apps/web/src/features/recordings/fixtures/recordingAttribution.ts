import type { components } from "../../../generated";
import type { MetadataObservation, RecordingDetailsPageProps } from "../../../shared/types";
import { sourceAttachmentPage } from "../../sources/fixtures/sourceAttachment";

export const selectedRecordingAttribution = {
  creators: [
    "Owned first creator",
    "Owned second creator",
    "Owned first creator",
  ],
  provider_label: "Owned recording catalogue",
  content_url: "https://credits.example.invalid/recordings/selected",
} satisfies components["schemas"]["RecordingAttribution"];

const original = sourceAttachmentPage.detail.selected_observation;
if (!original) throw new Error("The owned recording needs provider metadata");

export const uncreditedRecordingObservation = original;
export const selectedRecordingObservation: MetadataObservation = {
  ...original,
  attribution: selectedRecordingAttribution,
};

export const historicalRecordingAttribution = {
  creators: ["Owned earlier creator", "Owned earlier collaborator"],
  provider_label: "Owned earlier catalogue",
  content_url: "https://credits.example.invalid/recordings/earlier",
} satisfies components["schemas"]["RecordingAttribution"];

export const historicalRecordingObservation: MetadataObservation = {
  ...original,
  id: "0dd52f94-cd8e-47ce-bb32-d6cd29ac5204",
  title: "Owned earlier observed title",
  artist: "Owned earlier observed artist",
  observed_at: 0,
  provider_revision: "owned-earlier-revision",
  attribution: historicalRecordingAttribution,
};

export const attributedRecordingPage: RecordingDetailsPageProps = {
  ...sourceAttachmentPage,
  detail: {
    ...sourceAttachmentPage.detail,
    selected_observation: selectedRecordingObservation,
    observations: { items: [selectedRecordingObservation], next_before: null },
  },
};

export const historicalRecordingPage: RecordingDetailsPageProps = {
  ...attributedRecordingPage,
  detail: {
    ...attributedRecordingPage.detail,
    observations: {
      items: [selectedRecordingObservation, historicalRecordingObservation],
      next_before: null,
    },
  },
};
