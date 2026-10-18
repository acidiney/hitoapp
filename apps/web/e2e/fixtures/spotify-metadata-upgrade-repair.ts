import { isDeepStrictEqual } from "node:util";
import { liveCheck, normalizedLiveRecording } from "./spotify-albums-live";
import type {
  UpgradeLive,
  UpgradeSchema,
} from "./spotify-metadata-upgrade-state";
import type { PreparedSpotifyUpgrade } from "./spotify-metadata-upgrade-setup";
import type { spotifyUpgradeState } from "./spotify-metadata-upgrade-state";

type State = Awaited<ReturnType<typeof spotifyUpgradeState>>;

function copyIdentity(copy: UpgradeSchema["PlaylistDetail"]) {
  return {
    playlist: copy.playlist,
    origin: copy.origin,
    follow_content_revision: copy.follow_content_revision,
    entries: copy.entries.map((entry) => [
      entry.entry_id,
      entry.recording_id,
      entry.position,
      entry.origin,
    ]),
  };
}

export function verifyLiveSpotifyRepair(
  before: State,
  after: State,
  prepared: PreparedSpotifyUpgrade,
  current: UpgradeLive,
  receipt: UpgradeSchema["ImportedPlaylistMetadataRefresh"],
) {
  liveCheck(
    isDeepStrictEqual(copyIdentity(after.copy), copyIdentity(before.copy)) &&
      isDeepStrictEqual(after.user, before.user) &&
      isDeepStrictEqual(after.settings, before.settings) &&
      isDeepStrictEqual(after.playlists, before.playlists) &&
      isDeepStrictEqual(after.sources, before.sources) &&
      isDeepStrictEqual(after.choice, before.choice),
  );
  liveCheck(
    after.events.length === before.events.length + 1 &&
      isDeepStrictEqual(
        after.events.slice(0, before.events.length),
        before.events,
      ),
  );
  const event = after.events.at(-1);
  liveCheck(
    event &&
      "publication" in event &&
      event.publication.kind === "playlist_metadata_refreshed",
  );
  const publication = event.publication;
  liveCheck(
    publication.playlist_id === prepared.playlist &&
      !Object.hasOwn(publication, "entries") &&
      !Object.hasOwn(publication, "name") &&
      publication.origin.installation_id === prepared.connection.id &&
      publication.origin.installation_revision ===
        prepared.connection.revision &&
      publication.origin.service_id === prepared.service.id &&
      publication.origin.service_revision === prepared.service.revision &&
      publication.origin.provider === "org.hito.spotify" &&
      publication.origin.provider_playlist_id === current.detail.id &&
      publication.origin.snapshot === current.detail.snapshot &&
      isDeepStrictEqual(
        publication.origin.attribution,
        current.detail.attribution,
      ),
  );
  const published = new Map(
    publication.recordings.map((item) => [item.recording_id, item]),
  );
  const releases = new Map<string, string>();
  let updated = 0;
  let unchanged = 0;
  let unavailable = 0;
  for (const [provider, original] of prepared.mapped.recordings) {
    const live = current.recordings.get(provider);
    const item = published.get(original.recording_id);
    if (!live) {
      unavailable++;
      liveCheck(!item);
      continue;
    }
    if (!live.release) {
      unchanged++;
      liveCheck(!item);
      continue;
    }
    updated++;
    liveCheck(
      item &&
        item.library_id === original.library_id &&
        item.release_id &&
        isDeepStrictEqual(normalizedLiveRecording(item.metadata), live) &&
        item.metadata.release?.artist == null &&
        item.observation_id !== original.observation_id,
    );
    const previous = releases.get(live.release.id);
    liveCheck(previous === undefined || previous === item.release_id);
    releases.set(live.release.id, item.release_id);
  }
  liveCheck(
    updated > 0 &&
      receipt.playlist_id === prepared.playlist &&
      receipt.updated_recordings === updated &&
      receipt.unchanged_recordings === unchanged &&
      receipt.unavailable_recordings === unavailable &&
      published.size === updated &&
      publication.recordings.length === updated &&
      new Set(releases.values()).size === releases.size &&
      published.has(prepared.selected),
  );
  liveCheck(
    after.catalog.length === before.catalog.length &&
      after.albums.total === releases.size,
  );
  const old = new Map(before.catalog.map((item) => [item.id, item]));
  for (const recording of after.catalog) {
    const previous = old.get(recording.id);
    liveCheck(previous);
    const metadata = published.get(recording.id);
    const personal = recording.id === prepared.selected;
    const wanted = metadata
      ? {
          ...previous,
          title: personal ? previous.title : metadata.metadata.title,
          artist: personal ? previous.artist : metadata.metadata.artist,
          inherited_title: metadata.metadata.title,
          inherited_artist: metadata.metadata.artist,
          duration_ms: metadata.metadata.duration_ms ?? 0,
          release_id: metadata.release_id,
          album: metadata.metadata.release?.title,
        }
      : previous;
    liveCheck(isDeepStrictEqual(recording, wanted));
  }
  const retainedProgress = (state: State) =>
    state.activity.history.map((item) => [
      item.id,
      item.position_ms,
      item.speed,
      item.updated_at,
      item.liked,
      item.available,
    ]);
  liveCheck(
    isDeepStrictEqual(retainedProgress(after), retainedProgress(before)),
  );
  for (const field of ["title", "artist"] as const)
    for (const value of ["value", "personal", "operation_id"] as const)
      liveCheck(after.metadata[field][value] === before.metadata[field][value]);
  const selected = published.get(prepared.selected);
  const observation = after.metadata.selected_observation;
  liveCheck(
    selected &&
      observation &&
      observation.id === selected.observation_id &&
      observation.release_id === selected.release_id &&
      observation.album_artist === null &&
      observation.source_id === null &&
      observation.origin === "provider_response" &&
      observation.provider === "org.hito.spotify" &&
      observation.plugin_installation_id === prepared.connection.id &&
      observation.provider_recording_id === prepared.selectedProvider &&
      after.metadata.title.inherited === selected.metadata.title &&
      after.metadata.artist.inherited === selected.metadata.artist &&
      after.metadata.observations.items.length ===
        before.metadata.observations.items.length + 1 &&
      before.metadata.observations.items.every((old) =>
        after.metadata.observations.items.some((item) =>
          isDeepStrictEqual(item, old),
        ),
      ),
  );
  return { published, releases, updated, unchanged, unavailable };
}
