import type { Schema } from "../../../shared/schema";
import type { Availability } from "../../../shared/ui/AvailabilityLabel";

export type TrackStatus = {
  state: Availability;
  label: string;
  pinned: boolean;
  playable: boolean | null;
};

type Entry = Schema<"SourceChoiceEntry">;

export function isLocalSource(source: Schema<"Source">): boolean {
  return source.provider === "local" || Boolean(source.device_id);
}

export function sourceName(source: Schema<"Source">): string {
  return isLocalSource(source) ? "Local file" : source.provider;
}

function active(entries: Entry[]): Entry | undefined {
  return (
    entries.find((entry) => entry.selected && entry.source.ready) ??
    entries.find((entry) => entry.source.ready)
  );
}

export function hasLocalFile(detail: Schema<"RecordingMetadata">): boolean {
  return detail.selected_observation?.origin === "file_inspection";
}

export function trackStatus(
  choice: Schema<"SourceChoice"> | null,
  unavailable: boolean,
  localFile = false,
): TrackStatus {
  if (choice === null && localFile)
    return {
      state: "library",
      label: "In library",
      pinned: false,
      playable: true,
    };
  if (choice === null)
    return {
      state: "catalog",
      label: "Catalog only",
      pinned: false,
      playable: unavailable ? false : null,
    };
  const pinned = choice.mode === "manual";
  const ready = active(choice.sources);
  if (!ready && localFile)
    return { state: "library", label: "In library", pinned, playable: true };
  if (!ready)
    return {
      state: "catalog",
      label: "Catalog only",
      pinned,
      playable: false,
    };
  return isLocalSource(ready.source)
    ? { state: "library", label: "In library", pinned, playable: true }
    : {
        state: "available",
        label: `Plays from ${ready.source.provider}`,
        pinned,
        playable: true,
      };
}
