import type { Schema } from "../../../shared/schema";

import {
  automaticChoice,
  choiceSourceA,
  choiceSourceB,
  currentManualChoice,
} from "./sourceChoice";
import { readyDownloadA } from "./sourceChoiceDownload";

export const initialChoiceA1: Schema<"SourceChoice"> = {
  ...automaticChoice,
  revision: 1,
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

export const currentChoiceB2: Schema<"SourceChoice"> = {
  ...currentManualChoice,
  revision: 2,
};

export const savedA1: Schema<"DownloadStatus"> = {
  ...readyDownloadA,
  choice_revision: 1,
};

export const previousA2: Schema<"DownloadStatus"> = {
  ...savedA1,
  choice_revision: 2,
  matches_source_choice: false,
};
