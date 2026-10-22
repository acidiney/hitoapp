import type { components } from "../../../generated";
import { automaticChoice, choiceSourceA, choiceSourceB } from "./sourceChoice";

export const unavailableManualChoice: components["schemas"]["SourceChoice"] = {
  ...automaticChoice,
  revision: 1,
  mode: "manual",
  sources: [
    {
      source: { ...choiceSourceA, ready: false },
      selected: false,
      selectable: true,
    },
    {
      source: { ...choiceSourceB, ready: false },
      selected: true,
      selectable: true,
    },
  ],
};

export const readyDownloadA: components["schemas"]["DownloadStatus"] = {
  status: "ready",
  intent_id: "ce893a45-bb08-455f-80c6-b4ffaf207613",
  recording_id: automaticChoice.recording_id,
  source_id: "8734217f-156c-4a37-8ff1-7c4049238254",
  host_device_id: "owned-download-host",
  bytes_received: 16_044,
  total_bytes: 16_044,
  error: null,
  choice_revision: 0,
  matches_source_choice: true,
};

export const previousDownloadA: components["schemas"]["DownloadStatus"] = {
  ...readyDownloadA,
  choice_revision: 1,
  matches_source_choice: false,
};

export const readyDownloadB: components["schemas"]["DownloadStatus"] = {
  ...readyDownloadA,
  intent_id: "a856bfe2-6f1c-44c3-a1c2-073143726e99",
  source_id: "b429ad99-b27a-483b-940e-74f7f18ae9d6",
  choice_revision: 1,
  matches_source_choice: true,
};
