import type { Schema } from "../../src/shared/schema";

const RECORDING_METADATA_ADDED = [
  "appears_in",
  "audio_format",
  "credited_artists",
  "last_played",
  "musicbrainz_recording_id",
  "release_facts",
] as const satisfies readonly (keyof Schema<"RecordingMetadata">)[];
const METADATA_SETTINGS_ADDED = [
  "automatic_artist_lookup",
  "automatic_artwork",
  "automatic_service_search",
] as const satisfies readonly (keyof Schema<"MetadataSettings">)[];
const PLAYLIST_ENTRY_ADDED = [
  "album",
  "artist_id",
  "credited_artists",
  "plays_from_services",
  "release_id",
] as const satisfies readonly (keyof Schema<"PlaylistEntry">)[];
const RECORDING_ADDED = [
  "artist_id",
  "credited_artists",
  "plays_from_services",
] as const satisfies readonly (keyof Schema<"Recording">)[];
const SOURCE_ADDED = [
  "match_kind",
  "provider_label",
] as const satisfies readonly (keyof Schema<"Source">)[];
const INSPECTED_TAGS_ADDED = [
  "audio",
  "date",
  "genre",
  "isrc",
  "label",
  "musicbrainz_album_id",
  "musicbrainz_recording_id",
  "musicbrainz_release_group_id",
  "musicbrainz_release_track_id",
  "track_total",
] as const satisfies readonly (keyof Schema<"InspectedTags">)[];

function without<T extends object>(
  value: T,
  added: readonly string[],
): Partial<T> {
  return Object.fromEntries(
    Object.entries(value).filter(([key]) => !added.includes(key)),
  ) as Partial<T>;
}

export function legacySource(source: Schema<"Source">) {
  return without(source, SOURCE_ADDED);
}

export function legacyPlaylistDetail(detail: Schema<"PlaylistDetail">) {
  return {
    ...detail,
    entries: detail.entries.map((entry) =>
      without(entry, PLAYLIST_ENTRY_ADDED),
    ),
  };
}

export function legacyRecordingPage(page: Schema<"RecordingPage">) {
  return {
    ...page,
    items: page.items.map((recording) => without(recording, RECORDING_ADDED)),
  };
}

function legacyObservation(observation: Schema<"MetadataObservation">) {
  return observation.tags
    ? { ...observation, tags: without(observation.tags, INSPECTED_TAGS_ADDED) }
    : observation;
}

export function legacyRecordingMetadata(metadata: Schema<"RecordingMetadata">) {
  const view = without(metadata, RECORDING_METADATA_ADDED);
  return {
    ...view,
    observations: {
      ...metadata.observations,
      items: metadata.observations.items.map(legacyObservation),
    },
    ...(metadata.selected_observation
      ? {
          selected_observation: legacyObservation(
            metadata.selected_observation,
          ),
        }
      : {}),
  };
}

export function legacySourceChoice(choice: Schema<"SourceChoice">) {
  return {
    ...choice,
    sources: choice.sources.map((entry) => ({
      ...entry,
      source: legacySource(entry.source),
    })),
  };
}

export function legacyMetadataSettings(settings: Schema<"MetadataSettings">) {
  return without(settings, METADATA_SETTINGS_ADDED);
}

export function legacySourceResolution(resolution: Schema<"SourceResolution">) {
  return { ...resolution, sources: resolution.sources.map(legacySource) };
}

type LegacyLocal = {
  catalog: Schema<"RecordingPage">;
  playlist: Schema<"PlaylistDetail">;
  metadata: Schema<"RecordingMetadata">[];
  sources: Schema<"Source">[][];
};

export function legacyLocalState<T extends LegacyLocal>(local: T) {
  return {
    ...local,
    catalog: legacyRecordingPage(local.catalog),
    playlist: legacyPlaylistDetail(local.playlist),
    metadata: local.metadata.map(legacyRecordingMetadata),
    sources: local.sources.map((list) => list.map(legacySource)),
  };
}

type LegacyOutage = {
  local: LegacyLocal;
  copies: Schema<"PlaylistDetail">[];
  metadata: Schema<"RecordingMetadata">[];
  choices: Schema<"SourceChoice">[];
  settings: Schema<"MetadataSettings">;
};

export function legacyOutageState<T extends LegacyOutage>(state: T) {
  return {
    ...state,
    local: legacyLocalState(state.local),
    copies: state.copies.map(legacyPlaylistDetail),
    metadata: state.metadata.map(legacyRecordingMetadata),
    choices: state.choices.map(legacySourceChoice),
    settings: legacyMetadataSettings(state.settings),
  };
}
