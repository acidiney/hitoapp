import type { Action, RecordingMetadata, ReleaseMetadata } from "./types";

export type MetadataTarget = { kind: "recording" | "release"; id: string };
export type MetadataField = "title" | "artist";
export type MetadataEditorInput =
  | {
      kind: "recording";
      id: string;
      metadata: Pick<RecordingMetadata, "title" | "artist">;
    }
  | {
      kind: "release";
      id: string;
      metadata: Pick<ReleaseMetadata, "title" | "artist">;
    };

export function metadataAction(
  target: MetadataTarget,
  field: MetadataField,
  personal: string | null,
): Action {
  if (target.kind === "recording") {
    const recording_id = target.id;
    if (field === "title")
      return personal === null
        ? { kind: "recording_title_inherit", recording_id }
        : { kind: "recording_title_set", recording_id, title: personal };
    return personal === null
      ? { kind: "recording_artist_inherit", recording_id }
      : { kind: "recording_artist_set", recording_id, artist: personal };
  }
  const release_id = target.id;
  if (field === "title")
    return personal === null
      ? { kind: "release_title_inherit", release_id }
      : { kind: "release_title_set", release_id, title: personal };
  return personal === null
    ? { kind: "release_artist_inherit", release_id }
    : { kind: "release_artist_set", release_id, artist: personal };
}
