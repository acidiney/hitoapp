import { isDeepStrictEqual } from "node:util";
import type { APIRequestContext } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";
import { liveCheck } from "./spotify-albums-live";

type AlbumRead = <T>(path: string) => Promise<T>;
type PublishedRecordings = ReadonlyMap<
  string,
  Schema<"ImportedPlaylistRecording">
>;

export class SpotifyAlbumsProjectionReader {
  requests = 0;
  lastStatus = 0;
  private bytes = 0;

  constructor(private readonly request: APIRequestContext) {}

  readonly read = async <T>(path: string): Promise<T> => {
    liveCheck(++this.requests <= 256);
    this.lastStatus = 0;
    const response = await this.request.get(path, {
      timeout: 15_000,
      maxRedirects: 0,
      maxRetries: 0,
    });
    try {
      this.lastStatus = response.status();
      liveCheck(response.status() === 200);
      const body = await response.body();
      this.bytes += body.length;
      liveCheck(
        body.length <= 32 * 1024 * 1024 && this.bytes <= 128 * 1024 * 1024,
      );
      return JSON.parse(body.toString()) as T;
    } finally {
      await response.dispose();
    }
  };
}

export async function verifySpotifyStoredRecordings(
  read: AlbumRead,
  publications: PublishedRecordings,
  total: number,
): Promise<Schema<"Recording">[]> {
  const catalog: Schema<"Recording">[] = [];
  for (let offset = 0; offset < total; offset += 200) {
    const result = await read<Schema<"RecordingPage">>(
      `/api/v1/recordings?limit=200&offset=${offset}`,
    );
    liveCheck(
      result.total === total &&
        result.offset === offset &&
        result.limit === 200 &&
        result.items.length > 0,
    );
    catalog.push(...result.items);
  }
  liveCheck(
    catalog.length === total &&
      new Set(catalog.map((item) => item.id)).size === total,
  );
  for (const recording of catalog) {
    const published = publications.get(recording.id);
    liveCheck(published);
    const metadata = published.metadata;
    liveCheck(
      recording.title === metadata.title &&
        recording.inherited_title === metadata.title &&
        recording.artist === metadata.artist &&
        recording.inherited_artist === metadata.artist &&
        recording.duration_ms === (metadata.duration_ms ?? 0) &&
        !recording.available &&
        !recording.liked &&
        recording.library_id === published.library_id &&
        (recording.release_id ?? null) === (published.release_id ?? null) &&
        (recording.album ?? null) === (metadata.release?.title ?? null),
    );
  }
  return catalog;
}

export async function verifySpotifyStoredAlbums(
  read: AlbumRead,
  publications: PublishedRecordings,
  releases: ReadonlyMap<string, string>,
): Promise<Schema<"Release">[]> {
  const albumTracks = new Map<string, Schema<"ImportedPlaylistRecording">[]>();
  for (const recording of publications.values()) {
    if (recording.release_id) {
      const tracks = albumTracks.get(recording.release_id) ?? [];
      tracks.push(recording);
      albumTracks.set(recording.release_id, tracks);
    }
  }
  const albums: Schema<"Release">[] = [];
  for (let offset = 0; offset < releases.size; offset += 200) {
    const result = await read<Schema<"ReleasePage">>(
      `/api/v1/releases?limit=200&offset=${offset}`,
    );
    liveCheck(
      result.total === releases.size &&
        result.offset === offset &&
        result.limit === 200 &&
        result.items.length > 0,
    );
    albums.push(...result.items);
  }
  liveCheck(
    albums.length === releases.size &&
      new Set(albums.map((item) => item.id)).size === albums.length,
  );
  for (const album of albums) {
    const tracks = albumTracks.get(album.id) ?? [];
    liveCheck(tracks.length > 0);
    const metadata = tracks[0].metadata.release;
    liveCheck(
      metadata &&
        releases.get(metadata.id) === album.id &&
        album.title === metadata.title &&
        album.artist === (metadata.artist ?? null) &&
        (album.artist_id === null) === (album.artist === null) &&
        tracks.every(
          (item) =>
            item.library_id === album.library_id &&
            item.metadata.release?.id === metadata.id &&
            item.metadata.release.title === metadata.title &&
            (item.metadata.release.artist ?? null) ===
              (metadata.artist ?? null),
        ) &&
        album.track_count === tracks.length &&
        album.duration_ms ===
          tracks.reduce(
            (sum, item) => sum + (item.metadata.duration_ms ?? 0),
            0,
          ),
    );
  }
  return albums;
}

export async function verifySpotifyStoredArtists(
  read: AlbumRead,
  expected: ReadonlyMap<string, Schema<"ProviderRecording">>,
  catalog: readonly Schema<"Recording">[],
): Promise<Schema<"Artist">[]> {
  const performerNames = new Set(
    [...expected.values()].flatMap((recording) =>
      recording.release?.artist
        ? [recording.artist, recording.release.artist]
        : [recording.artist],
    ),
  );
  const libraryIds = new Set(catalog.map((recording) => recording.library_id));
  const artists: Schema<"Artist">[] = [];
  for (let offset = 0; offset < performerNames.size; offset += 200) {
    const result = await read<Schema<"ArtistPage">>(
      `/api/v1/artists?limit=200&offset=${offset}`,
    );
    liveCheck(
      result.total === performerNames.size &&
        result.offset === offset &&
        result.limit === 200 &&
        result.items.length > 0,
    );
    artists.push(...result.items);
  }
  liveCheck(
    artists.length === performerNames.size &&
      new Set(artists.map((item) => item.name)).size === performerNames.size &&
      artists.every(
        (artist) =>
          performerNames.has(artist.name) && libraryIds.has(artist.library_id),
      ),
  );
  return artists;
}

export function spotifyAlbumEditionChecks(
  detail: Schema<"ReleaseDetail">,
  album: Schema<"Release">,
  publications: PublishedRecordings,
) {
  const checks = {
    release_matches: isDeepStrictEqual(detail.release, album),
    track_total_matches: detail.tracks.total === album.track_count,
    has_tracks: detail.tracks.items.length > 0,
    recordings_known: true,
    release_appearances_match: true,
    titles_match: true,
    artists_match: true,
    album_titles_match: true,
    source_free: true,
    disc_numbers_match: true,
    track_numbers_match: true,
  };
  for (const track of detail.tracks.items) {
    const published = publications.get(track.id);
    checks.recordings_known &&= published !== undefined;
    checks.release_appearances_match &&=
      published?.release_id === album.id && track.release_id === album.id;
    checks.titles_match &&= track.title === published?.metadata.title;
    checks.artists_match &&= track.artist === published?.metadata.artist;
    checks.album_titles_match &&= track.album === album.title;
    checks.source_free &&= !track.available;
    checks.disc_numbers_match &&=
      (track.disc_number ?? null) === (published?.metadata.disc_number ?? null);
    checks.track_numbers_match &&=
      (track.track_number ?? null) ===
      (published?.metadata.track_number ?? null);
  }
  return checks;
}

export function verifySpotifyAlbumEdition(
  detail: Schema<"ReleaseDetail">,
  album: Schema<"Release">,
  publications: PublishedRecordings,
) {
  liveCheck(
    isDeepStrictEqual(detail.release, album) &&
      detail.tracks.total === album.track_count &&
      detail.tracks.items.length > 0,
  );
  for (const track of detail.tracks.items) {
    const published = publications.get(track.id);
    liveCheck(
      published &&
        published.release_id === album.id &&
        track.release_id === album.id &&
        track.title === published.metadata.title &&
        track.artist === published.metadata.artist &&
        track.album === album.title &&
        !track.available &&
        (track.disc_number ?? null) ===
          (published.metadata.disc_number ?? null) &&
        (track.track_number ?? null) ===
          (published.metadata.track_number ?? null),
    );
  }
}

export function verifySpotifyAlbumCredit(
  metadata: Schema<"ReleaseMetadata">,
  album: Schema<"Release">,
) {
  liveCheck(
    metadata.artist.value === album.artist &&
      metadata.artist.inherited === album.artist &&
      metadata.artist.personal === null &&
      metadata.title.value === album.title &&
      metadata.title.personal === null,
  );
}

export function verifySpotifyTrackCredit(
  metadata: Schema<"RecordingMetadata">,
  published: Schema<"ImportedPlaylistRecording">,
  album: Schema<"Release">,
  connection: Schema<"PluginInstallation">,
) {
  const observation = metadata.selected_observation;
  const expected = published.metadata;
  liveCheck(
    metadata.title.value === expected.title &&
      metadata.title.inherited === expected.title &&
      metadata.title.personal === null &&
      metadata.artist.value === expected.artist &&
      metadata.artist.inherited === expected.artist &&
      metadata.artist.personal === null &&
      observation?.origin === "provider_response" &&
      observation.provider === connection.manifest.id &&
      observation.plugin_installation_id === connection.id &&
      observation.provider_recording_id === expected.id &&
      observation.provider_revision === (expected.revision ?? null) &&
      observation.source_id === null &&
      observation.release_id === album.id &&
      observation.album === expected.release?.title &&
      observation.album_artist === (expected.release?.artist ?? null) &&
      observation.title === expected.title &&
      observation.artist === expected.artist &&
      observation.duration_ms === (expected.duration_ms ?? 0) &&
      observation.disc_number === (expected.disc_number ?? null) &&
      observation.track_number === (expected.track_number ?? null),
  );
}

export function verifySpotifyPerformerCredit(
  detail: Schema<"ArtistDetail">,
  performer: Schema<"Artist">,
  publications: PublishedRecordings,
  albums: readonly Schema<"Release">[],
) {
  liveCheck(
    isDeepStrictEqual(detail.artist, performer) &&
      detail.recordings.items.every(
        (recording) =>
          recording.artist === performer.name &&
          publications.has(recording.id) &&
          !recording.available,
      ) &&
      detail.releases.items.every((release) =>
        albums.some((known) => isDeepStrictEqual(known, release)),
      ),
  );
}
