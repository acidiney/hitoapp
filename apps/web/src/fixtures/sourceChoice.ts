import type { components } from "../generated";
import type { Source } from "../types";
import { savedRecording } from "./sourceAttachment";

export const choiceSourceA: Source = {
  id: "1fa4df60-a272-4168-98ce-e451278beb29",
  recording_id: savedRecording.id,
  provider: "plugin",
  label: "Owned source A",
  device_id: null,
  ready: true,
  can_prepare: false,
};

export const choiceSourceB: Source = {
  ...choiceSourceA,
  id: "2c0bbef6-d372-455e-85b7-e9722a2c2da5",
  label: "Owned source B",
};

export const automaticChoice: components["schemas"]["SourceChoice"] = {
  recording_id: savedRecording.id,
  revision: 0,
  mode: "automatic",
  sources: [
    { source: choiceSourceA, selected: false, selectable: true },
    { source: choiceSourceB, selected: false, selectable: true },
  ],
};

export const currentManualChoice: components["schemas"]["SourceChoice"] = {
  ...automaticChoice,
  revision: 3,
  mode: "manual",
  sources: [
    {
      source: { ...choiceSourceA, ready: false },
      selected: false,
      selectable: true,
    },
    { source: choiceSourceB, selected: true, selectable: true },
  ],
};
