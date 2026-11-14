import { isDeepStrictEqual } from "node:util";
import { expect, it } from "vitest";
import type { Schema } from "../../src/shared/schema";
import {
  legacyMetadataSettings,
  legacyPlaylistDetail,
  legacyRecordingMetadata,
  legacyRecordingPage,
  legacySourceChoice,
  legacySourceResolution,
} from "./providerOutageLegacyView";

const legacyEntry = {
  entry_id: "e1",
  position: 0,
  recording_id: "r1",
  title: "Owned title",
  inherited_title: "Owned title",
  artist: "Owned artist",
  inherited_artist: "Owned artist",
  duration_ms: 1000,
  available: true,
  liked: false,
};
const currentEntry = {
  ...legacyEntry,
  album: null,
  artist_id: null,
  credited_artists: null,
  plays_from_services: false,
  release_id: null,
};
const detail = (entries: object[]) =>
  ({
    playlist: { id: "p1", name: "Owned", kind: "local" },
    entries,
  }) as unknown as Schema<"PlaylistDetail">;

const legacySource = {
  id: "s1",
  recording_id: "r1",
  provider: "owned",
  label: "Owned source",
  ready: true,
  can_prepare: false,
  device_id: null,
};
const currentSource = {
  ...legacySource,
  match_kind: "exact",
  provider_label: "Owned",
};

const legacyTags = { title: "Owned title", artist: "Owned artist" };
const currentTags = { ...legacyTags, audio: null, genre: null, isrc: null };
const observation = (tags: object | null) => ({
  id: "o1",
  title: "Owned title",
  tags,
});
const metadata = (
  extra: object,
  tags: object | null,
  selected: object | null,
) =>
  ({
    recording_id: "r1",
    title: { value: "Owned title" },
    artist: { value: "Owned artist" },
    observations: { items: [observation(tags)], next_before: null },
    selected_observation: selected,
    ...extra,
  }) as unknown as Schema<"RecordingMetadata">;
const metadataAdded = {
  appears_in: [],
  audio_format: null,
  credited_artists: [],
  last_played: null,
  musicbrainz_recording_id: null,
  release_facts: null,
};

it("ignores only the audited additions in a playlist", () => {
  expect(
    isDeepStrictEqual(
      legacyPlaylistDetail(detail([legacyEntry])),
      legacyPlaylistDetail(detail([currentEntry])),
    ),
  ).toBe(true);
  expect(
    isDeepStrictEqual(
      legacyPlaylistDetail(detail([legacyEntry])),
      legacyPlaylistDetail(
        detail([{ ...currentEntry, title: "Changed title" }]),
      ),
    ),
  ).toBe(false);
  const { liked: _removed, ...withoutLiked } = currentEntry;
  expect(
    isDeepStrictEqual(
      legacyPlaylistDetail(detail([legacyEntry])),
      legacyPlaylistDetail(detail([withoutLiked])),
    ),
  ).toBe(false);
  expect(
    isDeepStrictEqual(
      legacyPlaylistDetail(detail([legacyEntry])),
      legacyPlaylistDetail(detail([{ ...currentEntry, unknown_field: 1 }])),
    ),
  ).toBe(false);
  const second = { ...legacyEntry, entry_id: "e2", recording_id: "r2" };
  expect(
    isDeepStrictEqual(
      legacyPlaylistDetail(detail([legacyEntry, second])),
      legacyPlaylistDetail(
        detail([
          { ...second, ...currentEntry, entry_id: "e2", recording_id: "r2" },
          currentEntry,
        ]),
      ),
    ),
  ).toBe(false);
});

it("ignores audited metadata additions but keeps nested legacy values", () => {
  const before = metadata({}, legacyTags, observation(legacyTags));
  expect(
    isDeepStrictEqual(
      legacyRecordingMetadata(before),
      legacyRecordingMetadata(
        metadata(metadataAdded, currentTags, observation(currentTags)),
      ),
    ),
  ).toBe(true);
  expect(
    isDeepStrictEqual(
      legacyRecordingMetadata(before),
      legacyRecordingMetadata(
        metadata(
          metadataAdded,
          { ...currentTags, title: "Changed title" },
          observation(currentTags),
        ),
      ),
    ),
  ).toBe(false);
  expect(
    isDeepStrictEqual(
      legacyRecordingMetadata(metadata({}, null, null)),
      legacyRecordingMetadata(metadata(metadataAdded, null, null)),
    ),
  ).toBe(true);
  expect(
    isDeepStrictEqual(
      legacyRecordingMetadata(metadata({}, null, null)),
      legacyRecordingMetadata(
        metadata(metadataAdded, null, observation(currentTags)),
      ),
    ),
  ).toBe(false);
});

it("ignores audited source additions in choices and replayed receipts", () => {
  const choice = (source: object) =>
    ({
      recording_id: "r1",
      revision: 1,
      mode: "automatic",
      sources: [{ source, selected: false, selectable: true }],
    }) as unknown as Schema<"SourceChoice">;
  expect(
    isDeepStrictEqual(
      legacySourceChoice(choice(legacySource)),
      legacySourceChoice(choice(currentSource)),
    ),
  ).toBe(true);
  expect(
    isDeepStrictEqual(
      legacySourceChoice(choice(legacySource)),
      legacySourceChoice(choice({ ...currentSource, ready: false })),
    ),
  ).toBe(false);
  const resolution = (sources: object[]) =>
    ({
      request_id: "q1",
      resolution_id: "z1",
      recording_id: "r1",
      outcome: "ready",
      sources,
    }) as unknown as Schema<"SourceResolution">;
  expect(
    isDeepStrictEqual(
      legacySourceResolution(resolution([legacySource])),
      legacySourceResolution(resolution([currentSource])),
    ),
  ).toBe(true);
  expect(
    isDeepStrictEqual(
      legacySourceResolution(resolution([legacySource])),
      legacySourceResolution(resolution([])),
    ),
  ).toBe(false);
});

it("ignores audited setting and recording additions only", () => {
  const settings = (extra: object) =>
    ({
      revision: 3,
      enabled: false,
      providers: [],
      ...extra,
    }) as unknown as Schema<"MetadataSettings">;
  const added = {
    automatic_artist_lookup: true,
    automatic_artwork: true,
    automatic_service_search: false,
  };
  expect(
    isDeepStrictEqual(
      legacyMetadataSettings(settings({})),
      legacyMetadataSettings(settings(added)),
    ),
  ).toBe(true);
  expect(
    isDeepStrictEqual(
      legacyMetadataSettings(settings({})),
      legacyMetadataSettings(settings({ ...added, revision: 4 })),
    ),
  ).toBe(false);
  const page = (items: object[]) =>
    ({
      items,
      total: items.length,
      offset: 0,
      limit: 50,
    }) as unknown as Schema<"RecordingPage">;
  const recording = { id: "r1", title: "Owned title", liked: false };
  const current = {
    ...recording,
    artist_id: null,
    credited_artists: null,
    plays_from_services: false,
  };
  expect(
    isDeepStrictEqual(
      legacyRecordingPage(page([recording])),
      legacyRecordingPage(page([current])),
    ),
  ).toBe(true);
  expect(
    isDeepStrictEqual(
      legacyRecordingPage(page([recording])),
      legacyRecordingPage(page([{ ...current, liked: true }])),
    ),
  ).toBe(false);
});
