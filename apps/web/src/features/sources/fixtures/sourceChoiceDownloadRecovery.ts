import type { components } from "../../../generated";
import { automaticChoice, choiceSourceA, choiceSourceB } from "./sourceChoice";
import { readyDownloadB } from "./sourceChoiceDownload";

export const automaticAfterReset: components["schemas"]["SourceChoice"] = {
  ...automaticChoice,
  revision: 4,
};

export const manualSourceA: components["schemas"]["SourceChoice"] = {
  ...automaticChoice,
  revision: 5,
  mode: "manual",
  sources: [
    { source: choiceSourceA, selected: true, selectable: true },
    {
      source: { ...choiceSourceB, ready: false },
      selected: false,
      selectable: true,
    },
  ],
};

export const recoveredDownloadB: components["schemas"]["DownloadStatus"] = {
  ...readyDownloadB,
  choice_revision: 5,
  matches_source_choice: false,
};
