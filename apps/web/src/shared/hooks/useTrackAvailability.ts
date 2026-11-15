import { router } from "@inertiajs/react";
import { log } from "../log";
import type { Schema } from "../schema";
import type { ListboxOption } from "../ui/Listbox";
import { PAGING_PARAMETERS } from "./useCatalogSort";

export type TrackAvailability = Schema<"RecordingAvailability">;

export const ALL_TRACKS = "all";

export const AVAILABILITY_OPTIONS: ListboxOption[] = [
  { value: ALL_TRACKS, label: "All" },
  { value: "available_or_service", label: "Has a source" },
  { value: "no_source", label: "No source" },
];

export function chosenAvailability(): TrackAvailability | null {
  const asked = new URLSearchParams(window.location.search).get("availability");
  return asked === "available_or_service" || asked === "no_source"
    ? asked
    : null;
}

export function useTrackAvailability(path: string) {
  const active = chosenAvailability();
  function change(value: string) {
    log.info("navigation.filter", { route: path, mode: value });
    const parameters = new URLSearchParams(window.location.search);
    PAGING_PARAMETERS.forEach((name) => parameters.delete(name));
    if (value === ALL_TRACKS) parameters.delete("availability");
    else parameters.set("availability", value);
    router.get(path, Object.fromEntries(parameters), {
      preserveScroll: true,
      replace: true,
    });
  }
  return {
    value: active ?? ALL_TRACKS,
    active,
    options: AVAILABILITY_OPTIONS,
    onChange: change,
  };
}
