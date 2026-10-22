import type { Source, SourceChoice, SourcePreparationStatus } from "../shared/types";
import { savedRecording } from "./sourceAttachment";
import { automaticChoice, choiceSourceA, choiceSourceB } from "./sourceChoice";

export const preparableSourceB: Source = {
  ...choiceSourceB,
  ready: false,
  can_prepare: true,
};

export const initialEligibility: Source[] = [choiceSourceA, preparableSourceB];
export const chosenEligibility: Source[] = [
  { ...choiceSourceA, ready: false },
  preparableSourceB,
];

export const initialChoice: SourceChoice = {
  ...automaticChoice,
  sources: initialEligibility.map((source) => ({
    source,
    selected: false,
    selectable: true,
  })),
};

export const chosenSourceB: SourceChoice = {
  ...initialChoice,
  revision: 1,
  mode: "manual",
  sources: chosenEligibility.map((source) => ({
    source,
    selected: source.id === preparableSourceB.id,
    selectable: true,
  })),
};

export const failedPreparation: SourcePreparationStatus = {
  preparation_id: "608f8886-0911-4543-9229-34cf58c621fa",
  recording_id: savedRecording.id,
  source_id: preparableSourceB.id,
  phase: "failed",
  reason: "source_unavailable",
  progress_basis_points: null,
  eta_seconds: null,
  next_check_at: null,
  created_at: 1,
  updated_at: 1,
};
