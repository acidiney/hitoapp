import { isDeepStrictEqual } from "node:util";
import { expect, type Page } from "@playwright/test";
import { liveCheck } from "./spotify-albums-live";
import { verifySpotifyAlbumCredit } from "./spotify-albums-projections";
import type {
  UpgradeRead,
  UpgradeSchema,
} from "./spotify-metadata-upgrade-state";
import type { PreparedSpotifyUpgrade } from "./spotify-metadata-upgrade-setup";
import type { spotifyUpgradeState } from "./spotify-metadata-upgrade-state";

type State = Awaited<ReturnType<typeof spotifyUpgradeState>>;

export function verifyLegacySpotifyState(
  before: State,
  prepared: PreparedSpotifyUpgrade,
) {
  liveCheck(
    isDeepStrictEqual(before.user, prepared.user) &&
      before.settings.installations.length === 1 &&
      before.settings.services.length === 1 &&
      isDeepStrictEqual(
        before.settings.installations[0],
        prepared.connection,
      ) &&
      isDeepStrictEqual(before.settings.services[0], prepared.service) &&
      before.albums.total === 0 &&
      before.catalog.every(
        (item) =>
          item.release_id == null && item.album == null && !item.available,
      ) &&
      before.copy.playlist.name === prepared.personal.playlist &&
      before.copy.entries.at(-1)?.recording_id === prepared.selected &&
      before.copy.entries.at(-1)?.position === before.copy.entries.length + 10,
  );
  liveCheck(
    before.copy.entries.length === prepared.original.entries.length &&
      new Set(before.copy.entries.map((entry) => entry.entry_id)).size ===
        before.copy.entries.length &&
      isDeepStrictEqual(before.copy.origin, prepared.original.origin) &&
      isDeepStrictEqual(before.copy.playlist, {
        ...prepared.original.playlist,
        name: prepared.personal.playlist,
      }),
  );
  const originalEntries = new Map(
    prepared.original.entries.map((entry) => [entry.entry_id, entry]),
  );
  for (const entry of before.copy.entries) {
    const original = originalEntries.get(entry.entry_id);
    liveCheck(original);
    const selected = original.recording_id === prepared.selected;
    liveCheck(
      isDeepStrictEqual(entry, {
        ...original,
        position:
          original.entry_id === prepared.movedEntry
            ? prepared.original.entries.length + 10
            : original.position,
        title: selected ? prepared.personal.title : original.title,
        artist: selected ? prepared.personal.artist : original.artist,
        liked: selected ? true : original.liked,
      }),
    );
  }
  const selected = before.catalog.find((item) => item.id === prepared.selected);
  liveCheck(
    selected?.title === prepared.personal.title &&
      selected.artist === prepared.personal.artist &&
      selected.liked,
  );
  const metadata = before.metadata;
  liveCheck(
    metadata.title.personal === prepared.personal.title &&
      metadata.artist.personal === prepared.personal.artist &&
      metadata.title.operation_id === prepared.edits.operations[0].id &&
      metadata.artist.operation_id === prepared.edits.operations[1].id &&
      metadata.selected_observation?.release_id === null &&
      metadata.selected_observation.source_id === null &&
      metadata.selected_observation.origin === "provider_response" &&
      metadata.selected_observation.provider === "org.hito.spotify" &&
      metadata.selected_observation.plugin_installation_id ===
        prepared.connection.id &&
      metadata.selected_observation.provider_recording_id ===
        prepared.selectedProvider,
  );
  const progress = before.activity.history;
  liveCheck(
    progress.length === 1 &&
      progress[0].id === prepared.selected &&
      progress[0].position_ms === 4200 &&
      progress[0].speed === 1.25 &&
      progress[0].liked &&
      before.sources.length === 0,
  );
  const operations = before.events.flatMap((event) =>
    "operation" in event ? [event.operation] : [],
  );
  liveCheck(
    isDeepStrictEqual(operations, prepared.edits.operations) &&
      isDeepStrictEqual(before.events[0], prepared.importEvent) &&
      before.events.every(
        (event) =>
          !("operation" in event) ||
          event.device_id === prepared.edits.device_id,
      ),
  );
}

export async function browseRepairedSpotifyAlbum(
  page: Page,
  read: UpgradeRead,
  albums: UpgradeSchema["Release"][],
  after: State,
  publication: ReadonlyMap<string, UpgradeSchema["ImportedPlaylistRecording"]>,
) {
  liveCheck(
    albums.length > 0 &&
      albums.every(
        (album) => album.artist === null && album.artist_id === null,
      ),
  );
  const album = albums[0];
  await page
    .getByRole("navigation", { name: "Main navigation" })
    .getByRole("link", { name: "Albums", exact: true })
    .click();
  await page.locator(`a[href="/albums/${album.id}"]`).first().click();
  await expect
    .poll(
      async () =>
        (await page.getByRole("heading", { level: 1 }).textContent()) ===
        album.title,
      { timeout: 8000 },
    )
    .toBe(true);
  await page.getByText("Album artist not provided", { exact: true }).waitFor();
  liveCheck((await page.locator(".album-hero .credit-link").count()) === 0);
  const detail = await read<UpgradeSchema["ReleaseDetail"]>(
    `/api/v1/releases/${album.id}`,
  );
  liveCheck(
    isDeepStrictEqual(detail.release, album) &&
      detail.tracks.total === album.track_count &&
      detail.tracks.items.length > 0,
  );
  const catalog = new Map(after.catalog.map((item) => [item.id, item]));
  for (const track of detail.tracks.items) {
    const item = publication.get(track.id);
    liveCheck(
      item &&
        track.release_id === album.id &&
        track.title === catalog.get(track.id)?.title &&
        track.artist === catalog.get(track.id)?.artist &&
        !track.available &&
        (track.disc_number ?? null) === (item.metadata.disc_number ?? null) &&
        (track.track_number ?? null) === (item.metadata.track_number ?? null),
    );
  }
  const rows = page
    .getByRole("table", { name: "Tracks", exact: true })
    .locator("tbody tr");
  await expect
    .poll(async () => (await rows.count()) === detail.tracks.items.length, {
      timeout: 8000,
    })
    .toBe(true);
  liveCheck(
    isDeepStrictEqual(
      await rows.locator(".artist-column").allTextContents(),
      detail.tracks.items.map((track) => track.artist),
    ),
  );
  verifySpotifyAlbumCredit(
    await read<UpgradeSchema["ReleaseMetadata"]>(
      `/api/v1/releases/${album.id}/metadata`,
    ),
    album,
  );
  liveCheck(
    await page
      .locator("audio[data-hito-player]")
      .evaluate(
        (audio: HTMLAudioElement) =>
          audio.paused && audio.currentTime === 0 && !audio.currentSrc,
      ),
  );
}
